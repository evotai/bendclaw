/**
 * Read plain text from the system clipboard: pbpaste (macOS), wl-paste / xclip (Linux).
 * Needed when the terminal reports Cmd+V as a key event, so no bracketed paste arrives.
 */

import { execFile } from 'child_process'
import { promisify } from 'util'

const execFileAsync = promisify(execFile)

const MAX_TEXT_BYTES = 4 * 1024 * 1024

export async function getTextFromClipboard(): Promise<string | null> {
  const candidates: [string, string[]][] = process.platform === 'darwin'
    ? [['pbpaste', []]]
    : process.platform === 'linux'
      ? [['wl-paste', ['--no-newline']], ['xclip', ['-selection', 'clipboard', '-o']]]
      : []

  for (const [cmd, args] of candidates) {
    try {
      const { stdout } = await execFileAsync(cmd, args, {
        timeout: 3000,
        maxBuffer: MAX_TEXT_BYTES,
      })
      const text = typeof stdout === 'string' ? stdout : String(stdout)
      if (text.length > 0) return text
    } catch {
      continue
    }
  }
  return null
}
