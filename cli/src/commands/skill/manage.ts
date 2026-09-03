import { join } from 'path'
import { existsSync } from 'fs'

import { discardCheckout, fetchRepo, type Checkout, type FetchFn, type ProgressFn } from './fetch.js'
import { installUnit, readSourceRecord, removeDirs } from './install.js'
import { skillsRoot } from './paths.js'
import { missingRequirements } from './requires.js'
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

async function applyUnit(
  ctx: Context,
  source: Source,
  unit: Unit,
  commit: string,
): Promise<string[]> {
  const superseded = source.official ? supersededDirs(ctx.root, unit) : []
  await installUnit(unit, record(source, unit, commit), ctx.root)

  const lines: string[] = []
  if (superseded.length) {
    removeDirs(superseded.map((entry) => entry.dir))
    lines.push(`  removed superseded: ${superseded.map((entry) => entry.name).join(', ')}`)
  }

  const installed = unit.skills.map((skill) => ({
    ...skill,
    dir: skill.group ? join(ctx.root, unit.name, skill.name) : join(ctx.root, unit.name),
  }))
  lines.push(...missingRequirements(unit.name, installed, ctx.env, ctx.variablesFile))
  return lines
}

function unitSummary(unit: Unit): string {
  return unit.skills.length > 1 ? `${unit.name} (${unit.skills.length} skills)` : unit.name
}

export async function skillInstall(arg?: string, options: ManageOptions = {}): Promise<string> {
  const ctx = context(options)
  const source = resolveSource(arg, ctx.env)
  const checkout = await ctx.fetch(source, ctx.progress)

  try {
    const units = enumerateUnits(checkout.dir, source)
    ctx.progress?.('installing skills...', 'info')
    const lines: string[] = []
    for (const unit of units) {
      const notes = await applyUnit(ctx, source, unit, checkout.commit)
      lines.push(`  ✓ ${unitSummary(unit)}`, ...notes)
    }
    return lines.join('\n')
  } finally {
    await discardCheckout(checkout)
  }
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

export async function skillUpdate(arg?: string, options: ManageOptions = {}): Promise<string> {
  const ctx = context(options)
  const name = arg?.trim()
  if (name && !isValidSkillName(name)) return `  invalid skill name: ${name}`

  let units = installedUnits(ctx.root)
  if (name) {
    units = units.filter((unit) => unit.name === name)
    if (!units.length) return `  skill not installed: ${name}`
    if (!units[0]!.record) return `  ${name} has no install source (local); nothing to update`
  }

  const tracked = units.filter((unit) => unit.record)
  if (!tracked.length) return '  no updatable skills installed'

  const groups = new Map<string, { source: Source; names: Installed[] }>()
  for (const unit of tracked) {
    const source: Source = {
      repo: unit.record!.repo,
      ref: unit.record!.ref,
      path: unit.record!.path,
      official: isOfficialRepo(unit.record!.repo, ctx.env),
    }
    const key = `${source.repo}@${source.ref}`
    const group = groups.get(key) ?? { source: { ...source, path: undefined }, names: [] }
    group.names.push(unit)
    groups.set(key, group)
  }

  const lines: string[] = []
  for (const { source, names } of groups.values()) {
    let checkout: Checkout
    try {
      checkout = await ctx.fetch(source, ctx.progress)
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error)
      for (const unit of names) lines.push(`  failed    ${unit.name}  ${detail}`)
      continue
    }

    try {
      for (const installedUnit of names) {
        const previous = installedUnit.record!
        const unitSource: Source = { ...source, path: previous.path }
        let unit: Unit
        try {
          unit = enumerateUnits(checkout.dir, unitSource)[0]!
        } catch (error) {
          const detail = error instanceof Error ? error.message : String(error)
          lines.push(`  failed    ${installedUnit.name}  ${detail}`)
          continue
        }
        const notes = await applyUnit(ctx, unitSource, unit, checkout.commit)
        const status = previous.commit === checkout.commit ? 'unchanged' : 'updated  '
        const version =
          previous.commit === checkout.commit
            ? previous.commit
            : `${previous.commit} → ${checkout.commit}`
        lines.push(`  ${status} ${unitSummary(unit)}  ${version}`)
        lines.push(...notes)
      }
    } finally {
      await discardCheckout(checkout)
    }
  }

  for (const unit of units) {
    if (!unit.record) lines.push(`  skipped   ${unit.name}  local`)
  }
  return lines.join('\n')
}

export function skillRemove(name: string, root = skillsRoot()): string {
  const trimmed = name.trim()
  if (!isValidSkillName(trimmed)) return `  invalid skill name: ${trimmed}`

  const unitDir = join(root, trimmed)
  if (existsSync(unitDir)) {
    const skills = scanSkillDir(root).filter((entry) => entry.dir.startsWith(`${unitDir}/`))
    removeDirs([unitDir])
    return skills.length
      ? `  ✓ removed skill group: ${trimmed} (${skills.length} skills)`
      : `  ✓ removed skill: ${trimmed}`
  }

  const nested = scanSkillDir(root).find((entry) => entry.name === trimmed && entry.group)
  if (nested) {
    removeDirs([nested.dir])
    return `  ✓ removed skill: ${trimmed} (from group ${nested.group})`
  }
  return `  skill not found: ${trimmed}`
}
