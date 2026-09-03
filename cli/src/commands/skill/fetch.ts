import { join } from 'path'
import { tmpdir } from 'os'
import { mkdtemp, rm } from 'fs/promises'

import type { Source } from './source.js'

export interface Checkout {
  dir: string
  commit: string
}

export type ProgressFn = (msg: string, level: 'info' | 'warn' | 'error') => void
export type FetchFn = (source: Source, progress?: ProgressFn) => Promise<Checkout>

async function run(command: string[], action: string): Promise<string> {
  const proc = Bun.spawn(command, { stdout: 'pipe', stderr: 'pipe' })
  const [exitCode, stdout, stderr] = await Promise.all([
    proc.exited,
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ])
  if (exitCode !== 0) {
    const detail = stderr.trim()
    throw new Error(`${action} failed${detail ? `: ${detail}` : ''}`)
  }
  return stdout
}

async function githubToken(): Promise<string> {
  try {
    const proc = Bun.spawn(['gh', 'auth', 'token'], { stdout: 'pipe', stderr: 'ignore' })
    const [exitCode, stdout] = await Promise.all([proc.exited, new Response(proc.stdout).text()])
    return exitCode === 0 ? stdout.trim() : ''
  } catch {
    return ''
  }
}

export function commitFromRoot(listing: string): string {
  const first = listing.split('\n').find((line) => line.trim())?.trim() ?? ''
  const root = first.split('/')[0] ?? ''
  const commit = root.split('-').pop() ?? ''
  return /^[0-9a-f]{7,40}$/.test(commit) ? commit : 'unknown'
}

export async function fetchRepo(source: Source, progress?: ProgressFn): Promise<Checkout> {
  const dir = await mkdtemp(join(tmpdir(), 'evot-skill-'))
  try {
    const tarball = join(dir, 'repo.tar.gz')
    progress?.(`downloading ${source.repo}@${source.ref}...`, 'info')
    const token = await githubToken()
    const headers = token
      ? ['-H', `Authorization: token ${token}`, '-H', 'Accept: application/vnd.github+json']
      : ['-H', 'Accept: application/vnd.github+json']
    await run(
      [
        'curl',
        '-fsSL',
        ...headers,
        '-o',
        tarball,
        `https://api.github.com/repos/${source.repo}/tarball/${source.ref}`,
      ],
      'download repo',
    )

    progress?.('extracting archive...', 'info')
    const listing = await run(['tar', 'tzf', tarball], 'read tarball')
    await run(['tar', 'xzf', tarball, '--strip-components=1', '-C', dir], 'extract tarball')

    return { dir, commit: commitFromRoot(listing) }
  } catch (error) {
    await rm(dir, { recursive: true, force: true })
    throw error
  }
}

export async function discardCheckout(checkout: Checkout): Promise<void> {
  await rm(checkout.dir, { recursive: true, force: true })
}
