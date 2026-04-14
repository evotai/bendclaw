/**
 * Multiline input continuation detection.
 * Ported from Rust repl.rs needs_continuation.
 *
 * Detects unclosed triple-backtick fences and trailing backslash
 * to prevent premature submission.
 */

/**
 * Check if the input text needs continuation (should not be submitted yet).
 * Returns true if:
 * - There's an unclosed triple-backtick fence
 * - The last line ends with a trailing backslash
 */
export function needsContinuation(text: string): boolean {
  // Trailing backslash
  const lines = text.split('\n')
  const lastLine = lines[lines.length - 1] ?? ''
  if (lastLine.endsWith('\\')) return true

  // Unclosed triple-backtick fence
  let fenceOpen = false
  for (const line of lines) {
    const trimmed = line.trim()
    if (trimmed.startsWith('```')) {
      fenceOpen = !fenceOpen
    }
  }
  return fenceOpen
}
