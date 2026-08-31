/**
 * @file completion — async file path completion using fd.
 * Triggered when user types `@` in the input.
 *
 * Two modes:
 * - Browse: `@` or `@dir/` lists the direct children of the target directory.
 * - Fuzzy:  `@<query>` searches the whole tree recursively and ranks matches
 *   against the full relative path (codex-style), so `@repl` finds
 *   `cli/src/term/repl.ts` without navigating directory by directory.
 */

import { spawn } from 'child_process'
import { readdirSync, statSync } from 'fs'
import { homedir } from 'os'
import { join, basename, resolve } from 'path'

export interface FileCompletionItem {
  /** Display label (relative path, directories end with /) */
  label: string
  /** Value to insert (includes @ prefix) */
  value: string
  /** Whether this is a directory */
  isDirectory: boolean
}

export interface FileCompletionResult {
  items: FileCompletionItem[]
  /** The @ prefix being completed (e.g. "@src/") */
  prefix: string
  /** Start index of the prefix in the line */
  prefixStart: number
  /** Context line shown below file candidates when search scope needs explaining. */
  note?: string
}

/** Max items shown in the completion menu. */
const MAX_ITEMS = 20
/** Max candidates fetched from fd before ranking. */
const FD_FUZZY_FETCH = 400
/** Bounds for the no-fd recursive fallback walk. */
const WALK_MAX_ENTRIES = 2000
const WALK_MAX_DEPTH = 6
const WALK_SKIP = new Set(['node_modules', 'target', 'dist', '.git'])

/** Explain search scope only when it may surprise the user. */
export function fileCompletionNote(
  hasFd: boolean,
  browse: boolean,
  homeRoot: boolean,
): string | undefined {
  if (!hasFd && !browse) {
    return `files up to ${WALK_MAX_DEPTH} levels deep — install fd to search deeper`
  }
  if (homeRoot) {
    return 'searching home — open a project folder for more relevant results'
  }
  return undefined
}

/**
 * Extract the @-prefix from text before cursor.
 * Returns null if no @ completion should trigger.
 */
export function extractAtPrefix(textBeforeCursor: string): { prefix: string; start: number } | null {
  // Walk backwards to find the @ that starts this token
  for (let i = textBeforeCursor.length - 1; i >= 0; i--) {
    const ch = textBeforeCursor[i]!
    if (ch === '@') {
      // Must be at start of line or preceded by whitespace
      if (i === 0 || /\s/.test(textBeforeCursor[i - 1]!)) {
        return { prefix: textBeforeCursor.slice(i), start: i }
      }
      return null
    }
    // Stop at whitespace (no @ found in this token)
    if (/\s/.test(ch)) return null
  }
  return null
}

/**
 * An `@` query naming an absolute or `~` path anchors completion outside the
 * project. Browsing the named directory is the only correct reading: joining
 * such a query onto `cwd` produces a path that cannot exist, which is why these
 * queries used to return nothing at all.
 */
export interface AnchoredAtQuery {
  /** Directory listed for candidates. */
  dir: string
  /** Entry-name filter within `dir`; '' lists every entry. */
  filter: string
  /** Text before the entry name, used to rebuild the inserted value. */
  displayPrefix: string
}

/** Split an absolute or `~` query into a directory to list and a name filter. */
export function anchoredAtQuery(query: string, home = homedir()): AnchoredAtQuery | null {
  let expanded: string
  if (query === '~') expanded = `${home}/`
  else if (query.startsWith('~/')) expanded = home + query.slice(1)
  else if (query.startsWith('/')) expanded = query
  else return null

  const cut = expanded.lastIndexOf('/')
  return {
    // The root directory keeps its slash; every other parent drops it.
    dir: cut === 0 ? '/' : expanded.slice(0, cut),
    filter: expanded.slice(cut + 1),
    displayPrefix: expanded.slice(0, cut + 1),
  }
}

/**
 * List an anchored directory and rank its entries against the name filter.
 * Only one level is listed: an absolute anchor can point at a huge tree (`~`,
 * `/`), so recursing would stall completion for no gain.
 */
function browseAnchored(anchor: AnchoredAtQuery): FileCompletionItem[] {
  let entries: string[]
  try {
    entries = readdirSync(anchor.dir)
  } catch {
    return []
  }

  const scored: { name: string; score: number }[] = []
  for (const entry of entries) {
    // Dotfiles stay out of the way until the filter opts into them.
    if (entry.startsWith('.') && !anchor.filter.startsWith('.')) continue
    const score = anchor.filter === '' ? 0 : fuzzyScore(entry, anchor.filter)
    if (score === null) continue
    scored.push({ name: entry, score })
  }
  scored.sort((a, b) =>
    a.score - b.score || a.name.length - b.name.length || a.name.localeCompare(b.name))

  return scored.slice(0, MAX_ITEMS).map(({ name }) => {
    let isDirectory = false
    try { isDirectory = statSync(join(anchor.dir, name)).isDirectory() } catch { /* ignore */ }
    const label = anchor.displayPrefix + name + (isDirectory ? '/' : '')
    return { label, value: `@${label}`, isDirectory }
  })
}

/**
 * Score a relative path against a fuzzy query. Lower is better; null means no
 * match. Tiers: basename prefix (0), basename substring (1), path substring
 * (2), path subsequence (3).
 */
export function fuzzyScore(relPath: string, query: string): number | null {
  const q = query.toLowerCase()
  const p = relPath.toLowerCase()
  const base = basename(p.endsWith('/') ? p.slice(0, -1) : p)
  if (base.startsWith(q)) return 0
  if (base.includes(q)) return 1
  if (p.includes(q)) return 2
  if (isSubsequence(q, p)) return 3
  return null
}

function isSubsequence(needle: string, haystack: string): boolean {
  let i = 0
  for (const ch of haystack) {
    if (ch === needle[i]) i++
    if (i === needle.length) return true
  }
  return needle.length === 0
}

/** Rank candidate relative paths against a query and build completion items. */
function rankCandidates(paths: string[], query: string): FileCompletionItem[] {
  const scored: { path: string; score: number }[] = []
  for (const path of paths) {
    if (path === '.git' || path.startsWith('.git/')) continue
    const score = fuzzyScore(path, query)
    if (score !== null) scored.push({ path, score })
  }
  scored.sort((a, b) =>
    a.score - b.score || a.path.length - b.path.length || a.path.localeCompare(b.path))
  return scored.slice(0, MAX_ITEMS).map(({ path }) => ({
    label: path,
    value: `@${path}`,
    isDirectory: path.endsWith('/'),
  }))
}

/**
 * Find fd binary path.
 */
function findFd(): string | null {
  for (const name of ['fd', 'fdfind']) {
    const path = Bun.which(name)
    if (path) return path
  }

  // Some launchers pass a restricted PATH; keep common absolute fallbacks.
  const candidates = ['/opt/homebrew/bin/fd', '/usr/local/bin/fd', '/usr/bin/fd', '/usr/bin/fdfind']
  for (const p of candidates) {
    try {
      statSync(p)
      return p
    } catch { /* not found */ }
  }
  return null
}

let fdPath: string | null | undefined

function getFdPath(): string | null {
  if (fdPath === undefined) {
    fdPath = findFd()
  }
  return fdPath
}

/**
 * Async file completion.
 * - `@` / `@dir/` → browse: direct children of the target directory.
 * - `@query` → fuzzy: recursive search ranked against the full relative path.
 * Falls back to readdir / a bounded recursive walk if fd is not available.
 */
export async function completeAtFile(
  textBeforeCursor: string,
  cwd: string,
  signal?: AbortSignal,
): Promise<FileCompletionResult | null> {
  const extracted = extractAtPrefix(textBeforeCursor)
  if (!extracted) return null

  const { prefix, start } = extracted
  const query = prefix.slice(1) // strip leading @
  const browse = query === '' || query.endsWith('/')

  // Absolute and `~` queries name their own search root, so they bypass the
  // cwd-relative browse and fuzzy paths entirely.
  const anchor = anchoredAtQuery(query)
  if (anchor) {
    const anchoredItems = browseAnchored(anchor)
    if (anchoredItems.length === 0) return null
    return { items: anchoredItems, prefix, prefixStart: start }
  }

  const fd = getFdPath()
  const homeRoot = resolve(cwd) === resolve(homedir())
  const note = fileCompletionNote(fd !== null, browse, homeRoot)
  let items: FileCompletionItem[]

  if (browse) {
    const searchDir = query.slice(0, -1) // '' for root
    items = fd
      ? await browseDirWithFd(fd, searchDir, cwd, signal)
      : browseWithReaddir(query, cwd)
  } else {
    const candidates = fd
      ? await fuzzyCandidatesWithFd(fd, query, cwd, signal)
      : walkDir(cwd)
    items = rankCandidates(candidates, query)
  }

  if (items.length === 0 && !note) return null
  const displayedNote = items.length === 0 && note ? `no matches · ${note}` : note

  return {
    items,
    prefix,
    prefixStart: start,
    ...(displayedNote ? { note: displayedNote } : {}),
  }
}

/** Run fd and return its output lines (directories carry a trailing slash). */
function runFd(
  fdPath: string,
  args: string[],
  signal?: AbortSignal,
): Promise<string[]> {
  return new Promise((resolve) => {
    if (signal?.aborted) {
      resolve([])
      return
    }

    const child = spawn(fdPath, args, {
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    let stdout = ''
    let resolved = false

    const finish = (results: string[]) => {
      if (resolved) return
      resolved = true
      if (onAbort) signal?.removeEventListener('abort', onAbort)
      resolve(results)
    }

    const onAbort = () => {
      if (child.exitCode === null) {
        child.kill('SIGKILL')
      }
      finish([])
    }

    if (signal) {
      signal.addEventListener('abort', onAbort, { once: true })
    }

    child.stdout.setEncoding('utf-8')
    child.stdout.on('data', (chunk: string) => {
      stdout += chunk
    })
    child.on('error', () => finish([]))
    child.on('close', (code) => {
      if (signal?.aborted || code !== 0 || !stdout) {
        finish([])
        return
      }
      finish(stdout.trim().split('\n').filter(Boolean))
    })
  })
}

/** Browse mode: list the direct children of `searchDir` with fd. */
async function browseDirWithFd(
  fdPath: string,
  searchDir: string,
  cwd: string,
  signal?: AbortSignal,
): Promise<FileCompletionItem[]> {
  const searchBase = searchDir ? join(cwd, searchDir) : cwd
  const lines = await runFd(fdPath, [
    '--base-directory', searchBase,
    '--max-depth', '1',
    '--max-results', String(MAX_ITEMS),
    '--type', 'f',
    '--type', 'd',
    '--follow',
    '--exclude', '.git',
  ], signal)

  const dirPrefix = searchDir ? searchDir + '/' : ''
  return lines
    .filter(line => line !== '.git' && !line.startsWith('.git/'))
    .map(line => ({
      label: dirPrefix + line,
      value: `@${dirPrefix}${line}`,
      isDirectory: line.endsWith('/'),
    }))
}

/**
 * Fuzzy mode: fetch recursive candidates whose full path matches the query as
 * a subsequence (fd regex `a.*b.*c`); ranking happens in [`rankCandidates`].
 */
async function fuzzyCandidatesWithFd(
  fdPath: string,
  query: string,
  cwd: string,
  signal?: AbortSignal,
): Promise<string[]> {
  const pattern = query
    .split('')
    .map(ch => ch.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
    .join('.*')
  return runFd(fdPath, [
    '--base-directory', cwd,
    '--max-results', String(FD_FUZZY_FETCH),
    '--type', 'f',
    '--type', 'd',
    '--follow',
    '--exclude', '.git',
    '--full-path',
    '--ignore-case',
    pattern,
  ], signal)
}

/** Browse fallback without fd: list one directory level with readdir. */
function browseWithReaddir(query: string, cwd: string): FileCompletionItem[] {
  const dir = query ? join(cwd, query) : cwd
  let entries: string[]
  try {
    entries = readdirSync(dir)
  } catch {
    return []
  }

  return entries
    .filter(e => !e.startsWith('.'))
    .slice(0, MAX_ITEMS)
    .map(entry => {
      let isDir = false
      try { isDir = statSync(join(dir, entry)).isDirectory() } catch { /* ignore */ }
      const label = query + entry + (isDir ? '/' : '')
      return {
        label,
        value: `@${label}`,
        isDirectory: isDir,
      }
    })
}

/**
 * Fuzzy fallback without fd: bounded recursive walk collecting relative paths
 * (directories carry a trailing slash).
 */
function walkDir(cwd: string): string[] {
  const out: string[] = []
  const walk = (rel: string, depth: number) => {
    if (out.length >= WALK_MAX_ENTRIES || depth > WALK_MAX_DEPTH) return
    let entries: string[]
    try {
      entries = readdirSync(join(cwd, rel))
    } catch {
      return
    }
    for (const entry of entries) {
      if (out.length >= WALK_MAX_ENTRIES) return
      if (entry.startsWith('.') || WALK_SKIP.has(entry)) continue
      const relPath = rel ? `${rel}/${entry}` : entry
      let isDir = false
      try { isDir = statSync(join(cwd, relPath)).isDirectory() } catch { /* ignore */ }
      if (isDir) {
        out.push(relPath + '/')
        walk(relPath, depth + 1)
      } else {
        out.push(relPath)
      }
    }
  }
  walk('', 0)
  return out
}
