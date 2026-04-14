/**
 * GitHub issue reference auto-linking.
 * Detects owner/repo#123 patterns and renders as styled links.
 * Ported from Rust linkify.rs.
 */

import chalk from 'chalk'

/**
 * Replace `owner/repo#123` patterns with styled issue references.
 */
export function linkifyIssueRefs(input: string): string {
  let out = ''
  let cursor = 0

  while (cursor < input.length) {
    const match = findIssueRef(input, cursor)
    if (!match) {
      out += input.slice(cursor)
      break
    }
    const [start, end, repo, num] = match
    out += input.slice(cursor, start)
    out += chalk.cyan(`${repo}#${num}`)
    cursor = end
  }

  return out
}

function findIssueRef(input: string, offset: number): [number, number, string, string] | null {
  for (let i = offset; i < input.length; i++) {
    if (input[i] !== '#') continue

    // Parse number after #
    let numEnd = i + 1
    while (numEnd < input.length && input[numEnd]! >= '0' && input[numEnd]! <= '9') numEnd++
    if (numEnd === i + 1) continue
    const num = input.slice(i + 1, numEnd)

    // Parse repo before #
    let repoStart = i
    while (repoStart > 0 && isRepoChar(input[repoStart - 1]!)) repoStart--
    const repo = input.slice(repoStart, i)
    if (!isValidRepo(repo)) continue

    // Check prefix isn't part of a longer identifier
    if (repoStart > 0) {
      const prefix = input[repoStart - 1]!
      if (/[a-zA-Z0-9_.\/-]/.test(prefix)) continue
    }

    return [repoStart, numEnd, repo, num]
  }
  return null
}

function isRepoChar(ch: string): boolean {
  return /[a-zA-Z0-9_\-\/.]/.test(ch)
}

function isValidRepo(repo: string): boolean {
  const parts = repo.split('/')
  if (parts.length !== 2) return false
  const [owner, name] = parts
  if (!owner || !name) return false
  return /^[a-zA-Z0-9_-]+$/.test(owner) && /^[a-zA-Z0-9_\-.]+$/.test(name)
}
