/**
 * /skill command — install, list, remove skills.
 * Skills live in ~/.evotai/skills/<name>/ with a SKILL.md file.
 */

import { join } from 'path'
import { homedir } from 'os'
import {
  readdirSync, existsSync, readFileSync, rmSync, mkdirSync,
  cpSync, statSync,
} from 'fs'
import type { ForkedAgent } from '../native/index.js'

const SKILLS_DIRS = [
  join(homedir(), '.evotai', 'skills'),
  join(homedir(), '.claude', 'skills'),
]
const SKILLS_DIR = SKILLS_DIRS[0]!

// ---------------------------------------------------------------------------
// /skill list
// ---------------------------------------------------------------------------

export function skillList(): string {
  const entries = SKILLS_DIRS.flatMap((dir) => {
    if (!existsSync(dir)) return []
    return readdirSync(dir)
      .filter((name) => existsSync(join(dir, name, 'SKILL.md')))
      .map((name) => ({ name, dir: join(dir, name) }))
  }).sort((a, b) => a.name.localeCompare(b.name))

  if (entries.length === 0) return '  no skills installed'

  return `\n  Skills:\n${entries
    .map(({ name, dir }) => `  • [${name}] ${dir}`)
    .join('\n')}`
}

// ---------------------------------------------------------------------------
// /skill install <source>
// ---------------------------------------------------------------------------

export interface GitHubSource {
  repo: string
  gitRef?: string
  subpath?: string
}

export function parseGitHubSource(input: string): GitHubSource {
  const trimmed = input.trim()

  // Full URL: https://github.com/owner/repo/tree/ref/path
  const urlMatch = trimmed.match(
    /^https?:\/\/github\.com\/([^/]+\/[^/]+)(?:\/tree\/([^/]+)(?:\/(.+))?)?$/
  )
  if (urlMatch) {
    return {
      repo: urlMatch[1]!,
      gitRef: urlMatch[2],
      subpath: urlMatch[3],
    }
  }

  // Short form: owner/repo
  if (/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/.test(trimmed)) {
    return { repo: trimmed }
  }

  throw new Error(`Invalid source: ${trimmed}. Use owner/repo or a GitHub URL.`)
}

export function isValidSkillName(name: string): boolean {
  return /^[a-zA-Z0-9._-]+$/.test(name) && name.length <= 64
}

export type ProgressFn = (msg: string, level: 'info' | 'warn' | 'error') => void

export async function skillInstall(
  source: string,
  forkedAgent?: ForkedAgent,
  progress?: ProgressFn,
): Promise<string | null> {
  const parsed = parseGitHubSource(source)
  const repoName = parsed.repo.split('/')[1] ?? parsed.repo

  // Clone to temp dir
  const { mkdtempSync } = await import('fs')
  const { tmpdir } = await import('os')
  const tempDir = mkdtempSync(join(tmpdir(), 'evot-skill-'))

  try {
    // Download repo tarball via GitHub API (avoids git-remote-https issues)
    const gitRef = parsed.gitRef ?? 'main'
    const tarFile = join(tempDir, 'repo.tar.gz')
    const ghToken = Bun.spawnSync(['gh', 'auth', 'token'], { stdout: 'pipe', stderr: 'pipe' })
    const token = ghToken.exitCode === 0 ? ghToken.stdout.toString().trim() : ''
    const headers: string[] = token
      ? ['-H', `Authorization: token ${token}`, '-H', 'Accept: application/vnd.github+json']
      : ['-H', 'Accept: application/vnd.github+json']
    const download = Bun.spawnSync(
      ['curl', '-fsSL', ...headers, '-o', tarFile, `https://api.github.com/repos/${parsed.repo}/tarball/${gitRef}`],
      { stdout: 'pipe', stderr: 'pipe' },
    )
    if (download.exitCode !== 0) {
      throw new Error(`failed to download repo: ${download.stderr.toString()}`)
    }
    const extract = Bun.spawnSync(
      ['tar', 'xzf', tarFile, '--strip-components=1', '-C', tempDir],
      { stdout: 'pipe', stderr: 'pipe' },
    )
    if (extract.exitCode !== 0) {
      throw new Error(`failed to extract tarball: ${extract.stderr.toString()}`)
    }

    // Determine source dir
    let srcDir = tempDir
    if (parsed.subpath) {
      srcDir = join(tempDir, parsed.subpath)
      if (!existsSync(srcDir)) {
        throw new Error(`Subpath not found: ${parsed.subpath}`)
      }
    }

    const installed: string[] = []

    // Check if srcDir itself is a skill (has SKILL.md)
    if (existsSync(join(srcDir, 'SKILL.md'))) {
      const name = parsed.subpath?.split('/').pop() ?? repoName
      installSkillDir(srcDir, name)
      installed.push(name)
    } else {
      // Multi-skill repo: scan top-level subdirs
      const subdirs = readdirSync(srcDir).filter((d) => {
        const p = join(srcDir, d)
        return statSync(p).isDirectory() && existsSync(join(p, 'SKILL.md'))
      })
      if (subdirs.length === 0) {
        throw new Error('No SKILL.md found in repo or subdirectories.')
      }
      for (const d of subdirs) {
        installSkillDir(join(srcDir, d), d)
        installed.push(d)
      }
    }

    // Report installed skills
    for (const name of installed) {
      progress?.(`  ✓ installed skill: ${name}`, 'info')
    }

    // Post-install LLM analysis (single skill only)
    if (forkedAgent && installed.length === 1) {
      progress?.('  analyzing skill...', 'info')
      const guide = await printSetupGuide(forkedAgent, installed[0]!)
      if (guide) {
        progress?.(guide, 'info')
      }
    } else if (installed.length > 1) {
      progress?.(`  installed ${installed.length} skills; use the skill tool to explore each one`, 'info')
    }

    return null
  } finally {
    rmSync(tempDir, { recursive: true, force: true })
  }
}

function installSkillDir(srcDir: string, name: string): void {
  if (!isValidSkillName(name)) {
    throw new Error(`Invalid skill name: ${name}`)
  }
  const destDir = join(SKILLS_DIR, name)
  mkdirSync(destDir, { recursive: true })

  // Copy excluding .git
  const entries = readdirSync(srcDir)
  for (const entry of entries) {
    if (entry === '.git') continue
    const src = join(srcDir, entry)
    const dst = join(destDir, entry)
    cpSync(src, dst, { recursive: true })
  }
}

// ---------------------------------------------------------------------------
// /skill remove
// ---------------------------------------------------------------------------

export function skillRemove(name: string): string {
  if (!isValidSkillName(name)) {
    return `  invalid skill name: ${name}`
  }
  const skillDir = join(SKILLS_DIR, name)
  if (!existsSync(skillDir)) {
    return `  skill not found: ${name}`
  }
  rmSync(skillDir, { recursive: true, force: true })
  return `  ✓ removed skill: ${name}`
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function extractDescription(skillMdPath: string): string {
  try {
    const content = readFileSync(skillMdPath, 'utf-8')
    // Look for description in YAML frontmatter
    const fmMatch = content.match(/^---\n([\s\S]*?)\n---/)
    if (fmMatch) {
      const descMatch = fmMatch[1]!.match(/description:\s*(.+)/)
      if (descMatch) return descMatch[1]!.trim()
    }
    // Fallback: first non-empty, non-heading line
    for (const line of content.split('\n')) {
      const trimmed = line.trim()
      if (trimmed && !trimmed.startsWith('#') && !trimmed.startsWith('---')) {
        return trimmed.slice(0, 80)
      }
    }
  } catch { /* ignore */ }
  return '(no description)'
}

async function printSetupGuide(forked: ForkedAgent, skillName: string): Promise<string | null> {
  const skillDir = join(SKILLS_DIR, skillName)
  const context = collectSkillContext(skillDir)
  if (!context) return null

  const prompt = `Analyze this skill and provide a brief setup guide with:\n## Configuration\nWhat env vars or settings are needed.\n## Security\nAny security considerations.\n\n${context}`

  try {
    const stream = await forked.query(prompt)
    let text = ''
    for await (const event of stream) {
      if (event.kind === 'assistant_delta' && event.payload?.delta) {
        text += event.payload.delta as string
      }
    }
    return text || null
  } catch {
    return null
  }
}

function collectSkillContext(skillDir: string): string | null {
  const parts: string[] = []
  for (const file of ['SKILL.md', 'README.md', '.env.example', '.env.template']) {
    const p = join(skillDir, file)
    if (existsSync(p)) {
      try {
        const content = readFileSync(p, 'utf-8').slice(0, 4000)
        parts.push(`<${file}>\n${content}\n</${file}>`)
      } catch { /* ignore */ }
    }
  }
  return parts.length > 0 ? parts.join('\n\n') : null
}
