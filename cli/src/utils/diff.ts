/**
 * Diff rendering — colored line diffs for file edit tool results.
 * Ported from Rust diff.rs.
 */

import chalk from 'chalk'
import { structuredPatch } from 'diff'

export interface DiffResult {
  text: string
  linesAdded: number
  linesRemoved: number
}

/**
 * Compute a colored unified diff between old and new text.
 */
export function formatDiff(oldText: string, newText: string, filename = ''): DiffResult {
  const patch = structuredPatch(filename, filename, oldText, newText, '', '', { context: 3 })

  let linesAdded = 0
  let linesRemoved = 0
  const lines: string[] = []

  for (const hunk of patch.hunks) {
    lines.push(chalk.gray(`@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`))
    for (const line of hunk.lines) {
      if (line.startsWith('+')) {
        lines.push(chalk.green(line))
        linesAdded++
      } else if (line.startsWith('-')) {
        lines.push(chalk.red(line))
        linesRemoved++
      } else {
        lines.push(chalk.dim(line))
      }
    }
  }

  return {
    text: lines.join('\n'),
    linesAdded,
    linesRemoved,
  }
}

/**
 * Colorize a pre-computed unified diff string (git-style).
 */
export function colorizeUnifiedDiff(diff: string): string {
  return diff
    .split('\n')
    .map(line => {
      if (line.startsWith('+++') || line.startsWith('---')) return chalk.bold(line)
      if (line.startsWith('@@')) return chalk.gray(line)
      if (line.startsWith('+')) return chalk.green(line)
      if (line.startsWith('-')) return chalk.red(line)
      return chalk.dim(line)
    })
    .join('\n')
}
