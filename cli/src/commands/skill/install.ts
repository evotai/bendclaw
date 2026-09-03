import { join } from 'path'
import { existsSync, readdirSync, readFileSync, rmSync } from 'fs'
import { cp, mkdir, rename, rm, writeFile } from 'fs/promises'

import { skillsRoot } from './paths.js'
import { isValidSkillName } from './scan.js'
import { SOURCE_FILE, type SourceRecord } from './source.js'
import type { Unit } from './units.js'

export function readSourceRecord(unitDir: string): SourceRecord | null {
  const file = join(unitDir, SOURCE_FILE)
  if (!existsSync(file)) return null
  try {
    const record = JSON.parse(readFileSync(file, 'utf8')) as SourceRecord
    if (record.version !== 1) return null
    return typeof record.repo === 'string' && typeof record.path === 'string' ? record : null
  } catch {
    return null
  }
}

export async function installUnit(unit: Unit, record: SourceRecord, root = skillsRoot()): Promise<void> {
  if (!isValidSkillName(unit.name)) throw new Error(`Invalid skill name: ${unit.name}`)

  const suffix = `${process.pid}-${Date.now()}`
  const stageDir = join(root, `.${unit.name}.install-${suffix}`)
  const backupDir = join(root, `.${unit.name}.backup-${suffix}`)
  const destDir = join(root, unit.name)

  await mkdir(root, { recursive: true })
  await rm(stageDir, { recursive: true, force: true })
  await mkdir(stageDir, { recursive: true })

  let backedUp = false
  try {
    for (const entry of readdirSync(unit.dir)) {
      if (entry === '.git' || entry === SOURCE_FILE) continue
      await cp(join(unit.dir, entry), join(stageDir, entry), { recursive: true })
    }
    await writeFile(join(stageDir, SOURCE_FILE), `${JSON.stringify(record, null, 2)}\n`)

    if (existsSync(destDir)) {
      await rename(destDir, backupDir)
      backedUp = true
    }
    try {
      await rename(stageDir, destDir)
    } catch (error) {
      if (backedUp) await rename(backupDir, destDir)
      throw error
    }
    if (backedUp) await rm(backupDir, { recursive: true, force: true })
  } finally {
    await rm(stageDir, { recursive: true, force: true })
  }
}

export function removeDirs(dirs: string[]): void {
  for (const dir of dirs) rmSync(dir, { recursive: true, force: true })
}
