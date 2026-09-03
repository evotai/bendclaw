import { join } from 'path'
import { homedir } from 'os'

function expandHome(dir: string): string {
  if (dir === '~') return homedir()
  if (dir.startsWith('~/')) return join(homedir(), dir.slice(2))
  return dir
}

export function skillsRoot(): string {
  return join(homedir(), '.evotai', 'skills')
}

export function builtinSkillsRoot(): string {
  return join(homedir(), '.evotai', 'builtin-skills')
}

export function variablesFile(): string {
  return join(homedir(), '.evotai', 'variables.json')
}

export function resolveSkillsDirs(env: NodeJS.ProcessEnv = process.env): string[] {
  const dirs = [builtinSkillsRoot(), skillsRoot()]
  const extra = env.EVOT_SKILLS_DIRS
  if (extra) {
    for (const part of extra.split(':')) {
      const trimmed = part.trim()
      if (trimmed) dirs.push(expandHome(trimmed))
    }
  }
  dirs.push(join(homedir(), '.claude', 'skills'))
  return [...new Set(dirs)]
}
