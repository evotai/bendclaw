/**
 * Keyboard hint formatting for overlay footers.
 *
 * One place owns how a chord is spelled, so every surface that advertises a key
 * reads the same way: `↑/↓ to select · Enter to view · x to stop · Esc to close`.
 */

/** Separator between hint entries. */
export const HINT_SEPARATOR = ' · '

/**
 * Display names for named keys. Bare characters (`x`, `f`) are shown as typed,
 * so only keys whose names differ from their glyphs are listed.
 */
const KEY_LABELS: Record<string, string> = {
  up: '↑',
  down: '↓',
  left: '←',
  right: '→',
  enter: 'Enter',
  escape: 'Esc',
  tab: 'Tab',
  space: 'Space',
  backspace: 'Backspace',
  delete: 'Delete',
}

/** Spell one key, or a set of interchangeable keys as `↑/↓`. */
export function formatChord(keys: string | string[]): string {
  const list = typeof keys === 'string' ? [keys] : keys
  return list.map(key => KEY_LABELS[key] ?? key).join('/')
}

export interface Hint {
  /** The key, or interchangeable keys, that trigger the action. */
  keys: string | string[]
  /** Verb phrase completing "<key> to …". */
  action: string
}
