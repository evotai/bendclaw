/**
 * Multiline input continuation detection.
 */

export function needsContinuation(text: string): boolean {
  const lines = text.split('\n')
  const lastLine = lines[lines.length - 1] ?? ''
  if (lastLine.endsWith('\\')) return true

  let fenceOpen = false
  for (const line of lines) {
    const trimmed = line.trim()
    if (trimmed.startsWith('```')) {
      fenceOpen = !fenceOpen
    }
  }
  return fenceOpen
}
