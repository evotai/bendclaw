import { dirname } from 'path'

import { readSourceRecord } from './install.js'
import { builtinSkillsRoot, resolveSkillsDirs, skillsRoot } from './paths.js'
import { getSkillEntries, type SkillEntry } from './scan.js'

interface Row {
  label: string
  origin: string
  members: string[]
  dir?: string
}

function unitDirOf(entry: SkillEntry): string {
  return entry.group ? dirname(entry.dir) : entry.dir
}

export function skillListFromDirs(dirs: string[]): string {
  const entries = getSkillEntries(dirs)
  if (entries.length === 0) return '  no skills installed'

  const builtin = builtinSkillsRoot()
  const managed = skillsRoot()
  const byKey = new Map<string, Row>()

  for (const entry of entries) {
    const unitDir = unitDirOf(entry)
    const record = readSourceRecord(unitDir)
    const isBuiltin = unitDir.startsWith(builtin)
    const origin = isBuiltin
      ? 'builtin'
      : record
        ? `${record.repo}@${record.commit}`
        : unitDir.startsWith(managed)
          ? 'local'
          : 'external'

    const label = isBuiltin ? 'builtin' : entry.group ? `${entry.group}/` : entry.name
    const row = byKey.get(`${label}\u0000${origin}`) ?? {
      label,
      origin,
      members: [],
      dir: isBuiltin || record ? undefined : unitDir,
    }
    if (label !== entry.name) row.members.push(entry.name)
    byKey.set(`${label}\u0000${origin}`, row)
  }

  const rows = [...byKey.values()].sort((a, b) => a.label.localeCompare(b.label))
  const width = Math.max(...rows.map((row) => row.label.length))
  const lines = rows.map((row) => {
    const parts = [`  ${row.label.padEnd(width)}`, row.origin]
    if (row.members.length) parts.push(`(${row.members.length}) ${row.members.join(', ')}`)
    if (row.dir) parts.push(row.dir)
    return parts.join('  ')
  })
  return `\n  Skills:\n${lines.join('\n')}`
}

export function skillList(dirs?: string[]): string {
  return skillListFromDirs(dirs ?? resolveSkillsDirs())
}
