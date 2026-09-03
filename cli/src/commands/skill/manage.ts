import { join } from 'path'
import { existsSync } from 'fs'

import { discardCheckout, fetchRepo, type Checkout, type FetchFn, type ProgressFn } from './fetch.js'
import { installUnit, readSourceRecord, removeDirs } from './install.js'
import { skillsRoot } from './paths.js'
import { missingRequirements } from './requires.js'
import type { SkillOutcome, UnitNote, UnitResult } from './render.js'
import { isValidSkillName, scanSkillDir, subdirs } from './scan.js'
import { isOfficialRepo, resolveSource, type Source, type SourceRecord } from './source.js'
import { enumerateUnits, supersededDirs, type Unit } from './units.js'

export interface ManageOptions {
  root?: string
  fetch?: FetchFn
  progress?: ProgressFn
  env?: NodeJS.ProcessEnv
  variablesFile?: string
}

export interface OfficialSyncResult {
  installed: string[]
  updated: string[]
  unchanged: string[]
  skipped: string[]
}

interface Context {
  root: string
  fetch: FetchFn
  progress?: ProgressFn
  env: NodeJS.ProcessEnv
  variablesFile?: string
}

function context(options: ManageOptions): Context {
  return {
    root: options.root ?? skillsRoot(),
    fetch: options.fetch ?? fetchRepo,
    progress: options.progress,
    env: options.env ?? process.env,
    variablesFile: options.variablesFile,
  }
}

function record(source: Source, unit: Unit, commit: string): SourceRecord {
  return {
    version: 1,
    repo: source.repo,
    ref: source.ref,
    path: unit.path,
    commit,
    installedAt: new Date().toISOString(),
  }
}

/** Skills present in the managed root, for the closing count. */
function installedSkillCount(root: string): number {
  return scanSkillDir(root).length
}

async function applyUnit(
  ctx: Context,
  source: Source,
  unit: Unit,
  commit: string,
  replaceSuperseded = true,
): Promise<UnitNote[]> {
  const superseded = source.official && replaceSuperseded ? supersededDirs(ctx.root, unit) : []
  await installUnit(unit, record(source, unit, commit), ctx.root)

  const notes: UnitNote[] = []
  if (superseded.length) {
    removeDirs(superseded.map((entry) => entry.dir))
    notes.push({
      kind: 'info',
      text: `replaced standalone ${superseded.map((entry) => entry.name).join(', ')}`,
    })
  }

  const installed = unit.skills.map((skill) => ({
    ...skill,
    dir: skill.group ? join(ctx.root, unit.name, skill.name) : join(ctx.root, unit.name),
  }))
  for (const text of missingRequirements(installed, ctx.env, ctx.variablesFile)) {
    notes.push({ kind: 'warn', text })
  }
  return notes
}

export async function skillInstall(arg?: string, options: ManageOptions = {}): Promise<SkillOutcome> {
  const ctx = context(options)
  const source = resolveSource(arg, ctx.env)
  const checkout = await ctx.fetch(source, ctx.progress)

  try {
    const units = enumerateUnits(checkout.dir, source)
    ctx.progress?.('installing...', 'info')
    const results: UnitResult[] = []
    for (const unit of units) {
      const notes = await applyUnit(ctx, source, unit, checkout.commit)
      results.push({
        name: unit.name,
        skills: unit.skills.length,
        outcome: 'new',
        detail: '',
        notes,
      })
    }
    return {
      view: {
        title: 'Installed',
        source: `${source.repo}@${checkout.commit}`,
        units: results,
        total: installedSkillCount(ctx.root),
      },
    }
  } finally {
    await discardCheckout(checkout)
  }
}

let officialSyncInFlight: Promise<OfficialSyncResult> | null = null

/**
 * Reconcile the managed root with the complete official catalog.
 *
 * New official units are installed and previously managed official units are
 * updated. A local or third-party unit with the same directory name is left
 * untouched, so background maintenance never replaces user-owned content.
 */
export async function syncOfficialSkills(
  options: ManageOptions = {},
): Promise<OfficialSyncResult> {
  const ctx = context(options)
  const source = resolveSource(undefined, ctx.env)
  const checkout = await ctx.fetch(source, ctx.progress)
  const result: OfficialSyncResult = { installed: [], updated: [], unchanged: [], skipped: [] }

  try {
    const units = enumerateUnits(checkout.dir, source)
    for (const unit of units) {
      const destination = join(ctx.root, unit.name)
      const installed = existsSync(destination)
      const previous = installed ? readSourceRecord(destination) : null

      if (installed && (!previous || !isOfficialRepo(previous.repo, ctx.env))) {
        result.skipped.push(unit.name)
        continue
      }
      if (previous?.commit === checkout.commit) {
        result.unchanged.push(unit.name)
        continue
      }

      await applyUnit(ctx, source, unit, checkout.commit, false)
      if (previous) result.updated.push(unit.name)
      else result.installed.push(unit.name)
    }
    return result
  } finally {
    await discardCheckout(checkout)
  }
}

/** Share one background reconciliation when startup paths overlap. */
export function startOfficialSkillSync(options: ManageOptions = {}): Promise<OfficialSyncResult> {
  if (!officialSyncInFlight) {
    officialSyncInFlight = syncOfficialSkills(options).finally(() => {
      officialSyncInFlight = null
    })
  }
  return officialSyncInFlight
}

interface Installed {
  name: string
  dir: string
  record: SourceRecord | null
}

function installedUnits(root: string): Installed[] {
  if (!existsSync(root)) return []
  return subdirs(root).map((name) => {
    const dir = join(root, name)
    return { name, dir, record: readSourceRecord(dir) }
  })
}

/** Group tracked units by the repo@ref they came from, so each is fetched once. */
function groupBySource(
  units: Installed[],
  env: NodeJS.ProcessEnv,
): Array<{ source: Source; members: Installed[] }> {
  const groups = new Map<string, { source: Source; members: Installed[] }>()
  for (const unit of units) {
    const tracked = unit.record
    if (!tracked) continue
    const source: Source = {
      repo: tracked.repo,
      ref: tracked.ref,
      official: isOfficialRepo(tracked.repo, env),
    }
    const key = `${source.repo}@${source.ref}`
    const group = groups.get(key) ?? { source, members: [] }
    group.members.push(unit)
    groups.set(key, group)
  }
  return [...groups.values()]
}

async function updateOne(
  ctx: Context,
  source: Source,
  checkout: Checkout,
  installed: Installed,
): Promise<UnitResult> {
  const previous = installed.record!
  const unitSource: Source = { ...source, path: previous.path }
  let unit: Unit
  try {
    unit = enumerateUnits(checkout.dir, unitSource)[0]!
  } catch (error) {
    return {
      name: installed.name,
      skills: 1,
      outcome: 'failed',
      detail: error instanceof Error ? error.message : String(error),
      notes: [],
    }
  }
  const notes = await applyUnit(ctx, unitSource, unit, checkout.commit)
  const same = previous.commit === checkout.commit
  return {
    name: unit.name,
    skills: unit.skills.length,
    outcome: same ? 'unchanged' : 'updated',
    detail: same ? previous.commit : `${previous.commit} → ${checkout.commit}`,
    notes,
  }
}

export async function skillUpdate(arg?: string, options: ManageOptions = {}): Promise<SkillOutcome> {
  const ctx = context(options)
  const name = arg?.trim()
  if (name && !isValidSkillName(name)) return { notice: `invalid skill name: ${name}` }

  let units = installedUnits(ctx.root)
  if (name) {
    units = units.filter((unit) => unit.name === name)
    if (!units.length) return { notice: `skill not installed: ${name}` }
    if (!units[0]!.record) {
      return { notice: `${name} has no install source (local); nothing to update` }
    }
  }

  const groups = groupBySource(units, ctx.env)
  if (!groups.length) return { notice: 'no updatable skills installed' }

  const results: UnitResult[] = []
  for (const { source, members } of groups) {
    let checkout: Checkout
    try {
      checkout = await ctx.fetch(source, ctx.progress)
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error)
      for (const unit of members) {
        results.push({ name: unit.name, skills: 1, outcome: 'failed', detail, notes: [] })
      }
      continue
    }
    try {
      for (const unit of members) results.push(await updateOne(ctx, source, checkout, unit))
    } finally {
      await discardCheckout(checkout)
    }
  }

  for (const unit of units) {
    if (!unit.record) {
      results.push({ name: unit.name, skills: 1, outcome: 'skipped', detail: 'local', notes: [] })
    }
  }
  return { view: { title: 'Updated', units: results, total: installedSkillCount(ctx.root) } }
}

export function skillRemove(name: string, root = skillsRoot()): { notice: string; removed: boolean } {
  const trimmed = name.trim()
  if (!isValidSkillName(trimmed)) return { notice: `invalid skill name: ${trimmed}`, removed: false }

  const unitDir = join(root, trimmed)
  if (existsSync(unitDir)) {
    const skills = scanSkillDir(root).filter((entry) => entry.dir.startsWith(`${unitDir}/`))
    removeDirs([unitDir])
    return {
      notice: skills.length
        ? `removed skill group: ${trimmed} (${skills.length} skills)`
        : `removed skill: ${trimmed}`,
      removed: true,
    }
  }

  const nested = scanSkillDir(root).find((entry) => entry.name === trimmed && entry.group)
  if (nested) {
    removeDirs([nested.dir])
    return { notice: `removed skill: ${trimmed} (from group ${nested.group})`, removed: true }
  }
  return { notice: `skill not found: ${trimmed}`, removed: false }
}
