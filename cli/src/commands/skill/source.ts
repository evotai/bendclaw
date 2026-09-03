import { isValidSkillName } from './scan.js'

export const OFFICIAL_REPO = 'evotai/evot-skills'
export const OFFICIAL_URL = `https://github.com/${OFFICIAL_REPO}`
export const OFFICIAL_REF = 'main'
export const OFFICIAL_PREFIX = 'skills'
export const SOURCE_FILE = '.evot-source.json'

export interface Source {
  repo: string
  ref: string
  path?: string
  official: boolean
}

export interface SourceRecord {
  version: 1
  repo: string
  ref: string
  path: string
  commit: string
  installedAt: string
}

function officialRepo(env: NodeJS.ProcessEnv): string {
  return env.EVOT_SKILLS_REPO?.trim() || OFFICIAL_REPO
}

export function isOfficialRepo(repo: string, env: NodeJS.ProcessEnv = process.env): boolean {
  return repo === officialRepo(env)
}

export function parseGitHubSource(input: string): Source {
  const trimmed = input.trim()

  const urlMatch = trimmed.match(
    /^https?:\/\/github\.com\/([^/]+\/[^/]+?)(?:\.git)?(?:\/tree\/([^/]+)(?:\/(.+))?)?$/
  )
  if (urlMatch) {
    return {
      repo: urlMatch[1]!,
      ref: urlMatch[2] ?? 'main',
      path: urlMatch[3],
      official: false,
    }
  }

  const shortMatch = trimmed.match(/^([a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+)(?:@([^/]+))?$/)
  if (shortMatch) {
    return { repo: shortMatch[1]!, ref: shortMatch[2] ?? 'main', official: false }
  }

  throw new Error(`Invalid source: ${trimmed}. Use owner/repo or a GitHub URL.`)
}

export function resolveSource(arg?: string, env: NodeJS.ProcessEnv = process.env): Source {
  const trimmed = arg?.trim()
  if (!trimmed) {
    return { repo: officialRepo(env), ref: OFFICIAL_REF, official: true }
  }
  if (!trimmed.includes('/')) {
    if (!isValidSkillName(trimmed)) throw new Error(`Invalid skill name: ${trimmed}`)
    return {
      repo: officialRepo(env),
      ref: OFFICIAL_REF,
      path: `${OFFICIAL_PREFIX}/${trimmed}`,
      official: true,
    }
  }
  return parseGitHubSource(trimmed)
}
