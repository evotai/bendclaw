import { lstatSync, readFileSync } from 'fs'
import { join } from 'path'

export const DISPLAY_FILE = '.display.json'
const MAX_BYTES = 4096

/** Optional catalog presentation only; never used for skill selection. */
export interface SkillDisplay {
  summary: string
  example: string
}

export interface SkillDisplayResult {
  display?: SkillDisplay
  warning?: string
}

function validText(value: unknown, max: number): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= max &&
    value === value.trim() && /^[\x20-\x7e]+$/.test(value)
}

export function parseSkillDisplay(text: string, name: string): SkillDisplayResult {
  const invalid = { warning: `Invalid ${DISPLAY_FILE}` }
  if (Buffer.byteLength(text, 'utf8') > MAX_BYTES) return invalid
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch {
    return invalid
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid
  const data = value as Record<string, unknown>
  // Unversioned files (v0) are not supported by this new format.
  const version = data.schema_version === undefined ? 0 : data.schema_version
  if (version !== 1) {
    return { warning: `Unsupported ${DISPLAY_FILE} schema version` }
  }
  if (!validText(data.summary, 60) || !validText(data.example, 96) ||
      !data.example.startsWith(`${name}: `) || !data.example.slice(name.length + 2).trim()) {
    return invalid
  }
  return { display: { summary: data.summary, example: data.example } }
}

export function readSkillDisplay(dir: string, name: string): SkillDisplayResult {
  try {
    const file = join(dir, DISPLAY_FILE)
    const stat = lstatSync(file)
    if (!stat.isFile() || stat.size > MAX_BYTES) return { warning: `Invalid ${DISPLAY_FILE}` }
    return parseSkillDisplay(readFileSync(file, 'utf8'), name)
  } catch (error) {
    if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') return {}
    return { warning: `Cannot read ${DISPLAY_FILE}` }
  }
}
