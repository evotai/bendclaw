import { backgroundChordLabel } from '../design/key-hints.js'
import { line, block, plain, dim, bold, colored, type ViewBlock } from './types.js'

/** Pure help surface: independent of selectors, ask state and terminal I/O. */
export function buildHelpBlocks(_columns: number): ViewBlock[] {
  const TITLE = 'Keyboard Shortcuts & Commands'
  const DISMISS_HINT = 'Press Esc to dismiss'
  const INDENT = 2
  const KEY_GAP = 2
  const entries = [
    ['Enter', 'Submit message'],
    ['Alt+Enter', 'Insert newline'],
    ['Ctrl+C', 'Clear / Exit (×2)'],
    ['Esc', 'Clear input / Dismiss / Interrupt'],
    [backgroundChordLabel(), 'Run in background, keeping the work alive'],
    ['Ctrl+G', 'Focus queued prompts when non-empty'],
    ['↓ / Ctrl+T', 'Background task panel when shells are running'],
    ['↑ / ↓', 'History navigation / multi-line'],
    ['Tab', 'Complete command / path'],
    ['Ctrl+U', 'Clear line before cursor'],
    ['Ctrl+K', 'Clear line after cursor'],
    ['Ctrl+W', 'Delete word before cursor'],
    ['Ctrl+D', 'Delete char / Exit if empty'],
    ['Ctrl+A/E', 'Move to start/end of line'],
    ['Ctrl+L', 'Clear all input'],
    ['Ctrl+O', 'Expand/collapse output'],
    ['Shift+Tab', 'Cycle thinking level'],
    ['/help', 'Show this help'],
    ['/model <name>', 'Switch model'],
    ['/resume, /sessions', 'Resume session'],
    ['/new', 'Start new session'],
    ['/plan', 'Toggle planning mode'],
    ['/env', 'Manage variables'],
    ['/skill', 'Manage skills'],
    ['/copy', 'Copy last agent message (Markdown)'],
    ['/clip [all]', 'Clip last reply to vault; all = distill session'],
    ['/share [id|url]', 'Share or import a session'],
    ['/compact', 'Compact session context'],
    ['/version', 'Show current version'],
    ['/login', 'Log in to evot cloud'],
    ['/logout', 'Log out of evot cloud'],
    ['/clear', 'Clear session context'],
    ['/exit', 'Exit'],
  ]

  // Preserve the existing geometry: the terminal compositor centres this block.
  const maxKeyLen = Math.max(...entries.map(e => e[0]!.length))
  const descColumn = INDENT + maxKeyLen + KEY_GAP
  const blockWidth = Math.max(
    ...entries.map(e => descColumn + e[1]!.length),
    INDENT + TITLE.length,
  )
  const centred = (text: string): string =>
    ' '.repeat(Math.max(0, Math.floor((blockWidth - text.length) / 2)))
  return [block([
    line(bold(`${centred(TITLE)}${TITLE}`)),
    line(plain('')),
    ...entries.map(([key, desc]) => line(
      colored(`${' '.repeat(INDENT)}${key!.padEnd(maxKeyLen + KEY_GAP)}`, 'cyan'),
      dim(desc!),
    )),
    line(plain('')),
    line(dim(`${centred(DISMISS_HINT)}${DISMISS_HINT}`)),
  ], 1)]
}
