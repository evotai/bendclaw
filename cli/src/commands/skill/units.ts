import { join } from 'path'
import { existsSync } from 'fs'

import { OFFICIAL_PREFIX, type Source } from './source.js'
import { isValidSkillName, scanSkillDir, subdirs, type SkillEntry } from './scan.js'

export interface Unit {
  name: string
  dir: string
  path: string
  skills: SkillEntry[]
}

function unitAt(root: string, path: string, fallbackName?: string): Unit | null {
  const dir = join(root, path)
  const name = path.split('/').filter(Boolean).pop() ?? fallbackName ?? ''
  if (!name || !isValidSkillName(name)) return null

  if (existsSync(join(dir, 'SKILL.md'))) {
    return { name, dir, path, skills: [{ name, dir }] }
  }
  const skills = subdirs(dir)
    .filter((child) => existsSync(join(dir, child, 'SKILL.md')))
    .map((child) => ({ name: child, dir: join(dir, child), group: name }))
  return skills.length ? { name, dir, path, skills } : null
}

function unitsUnder(root: string, prefix: string): Unit[] {
  const base = prefix ? join(root, prefix) : root
  const units: Unit[] = []
  for (const child of subdirs(base)) {
    const unit = unitAt(root, prefix ? `${prefix}/${child}` : child)
    if (unit) units.push(unit)
  }
  return units.sort((a, b) => a.name.localeCompare(b.name))
}

export function enumerateUnits(root: string, source: Source): Unit[] {
  if (source.path) {
    const unit = unitAt(root, source.path)
    if (!unit) throw new Error(`No SKILL.md found at ${source.path}`)
    return [unit]
  }

  if (source.official) {
    const units = unitsUnder(root, OFFICIAL_PREFIX)
    if (!units.length) throw new Error(`No skills found under ${OFFICIAL_PREFIX}/ in ${source.repo}`)
    return units
  }

  if (existsSync(join(root, 'SKILL.md'))) {
    const fallback = source.repo.split('/')[1] ?? source.repo
    const unit = unitAt(root, '', fallback)
    if (unit) return [unit]
  }

  const nested = existsSync(join(root, OFFICIAL_PREFIX)) ? unitsUnder(root, OFFICIAL_PREFIX) : []
  const units = nested.length ? nested : unitsUnder(root, '')
  if (!units.length) throw new Error('No SKILL.md found in repo or subdirectories.')
  return units
}

export function supersededDirs(root: string, unit: Unit): SkillEntry[] {
  const names = new Set(unit.skills.map((skill) => skill.name))
  const unitDir = join(root, unit.name)
  return scanSkillDir(root).filter(
    (entry) => names.has(entry.name) && entry.dir !== join(unitDir, entry.name) && entry.dir !== unitDir,
  )
}
