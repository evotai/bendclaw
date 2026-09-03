import { join } from 'path'
import { existsSync, readFileSync } from 'fs'

import { parseRequires, type Requires } from './frontmatter.js'
import { variablesFile } from './paths.js'
import type { SkillEntry } from './scan.js'

function knownVariables(file: string): Set<string> {
  if (!existsSync(file)) return new Set()
  try {
    const doc = JSON.parse(readFileSync(file, 'utf8')) as {
      variables?: Array<{ key?: string }>
    }
    return new Set((doc.variables ?? []).map((item) => item.key).filter((key): key is string => Boolean(key)))
  } catch {
    return new Set()
  }
}

function mergeRequires(entries: SkillEntry[]): Requires {
  const merged: Requires = { env: [], bins: [], envHints: {} }
  for (const entry of entries) {
    const file = join(entry.dir, 'SKILL.md')
    if (!existsSync(file)) continue
    let parsed: Requires
    try {
      parsed = parseRequires(readFileSync(file, 'utf8'))
    } catch {
      continue
    }
    for (const name of parsed.env) if (!merged.env.includes(name)) merged.env.push(name)
    for (const bin of parsed.bins) if (!merged.bins.includes(bin)) merged.bins.push(bin)
    Object.assign(merged.envHints, parsed.envHints)
  }
  return merged
}

/**
 * What a freshly installed unit still needs from the user.
 *
 * Returns bare requirement phrases; the caller owns how they are labelled and
 * painted.
 */
export function missingRequirements(
  entries: SkillEntry[],
  env: NodeJS.ProcessEnv = process.env,
  varsFile: string = variablesFile(),
): string[] {
  const { env: needEnv, bins, envHints } = mergeRequires(entries)
  const variables = knownVariables(varsFile)
  const notes: string[] = []

  for (const name of needEnv) {
    if (env[name] || variables.has(name)) continue
    const hint = envHints[name]
    notes.push(`needs /env set ${name}=${hint ?? '<value>'}`)
  }
  for (const bin of bins) {
    if (Bun.which(bin)) continue
    notes.push(`needs ${bin} (not found in PATH)`)
  }
  return notes
}
