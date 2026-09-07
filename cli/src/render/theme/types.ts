/**
 * Theme contract shared by the dark and light palettes.
 *
 * All ANSI-styled text goes through `theme.<field>.paint(s)` so a theme swap
 * is a single-point change. Block fills (`*Bg`) are bare hex strings: the
 * viewmodel paints them across a whole padded row, which a per-span `Style`
 * cannot express.
 */

export interface Style {
  paint(text: string): string
}

export const plain: Style = { paint: s => s }

export function style(fn: (s: string) => string): Style {
  return { paint: fn }
}

export interface Theme {
  // Brand: periwinkle primary + gold structural accent.
  brand: Style
  brandBold: Style
  brandHex: string
  accent: Style
  accentBold: Style
  accentHex: string

  /** Secondary interface copy (hints, metadata), independent of ANSI dim. */
  mutedHex: string
  /** Fill behind a selected row (completion menu). */
  selectionBgHex: string
  /** Secondary text on `selectionBgHex`, where the normal dim gray is too dark. */
  selectionMutedHex: string
  /** The prompt caret. Deliberately off-palette so it reads as "you are here". */
  cursorHex: string
  /** Text sat on by the cursor block; contrasts against `cursorHex`. */
  cursorFgHex: string

  // Transcript blocks. The user message sits on `panelBg` behind a brand rail
  // (opencode's UserMessage). Tool cards share one neutral fill regardless of
  // lifecycle; the status glyph carries success/failure. Diff rows inside a
  // card swap the card fill for their own so +/- stay legible.
  panelBg: string
  /** User-message foregrounds paired with panelBg; never inherit terminal ink. */
  panelFg: string
  panelMutedFg: string
  toolCardBg: string
  diffAddedBg: string
  diffRemovedBg: string

  // Inline
  text: Style
  bold: Style
  italic: Style
  boldItalic: Style
  strikethrough: Style
  underline: Style
  link: Style
  codeInline: Style
  mathInline: Style
  mathBlock: Style

  // Headings (h1..h6)
  h1: Style
  h2: Style
  h3: Style
  h4: Style
  h5: Style
  h6: Style

  // Lists
  bullet: Style
  listNumber: Style

  // Blockquote
  blockquoteBorder: Style
  blockquoteText: Style

  // Table
  tableBorder: Style
  tableHeader: Style

  // Misc
  hr: Style
  codeBlockBorder: Style
  // Reasoning, opencode-style: an accent header (`Thinking…` / `Thought: …`)
  // over a muted body. Muted, not dim-italic: the body has to stay readable
  // for long CJK reasoning on a dark terminal.
  thinkHeader: Style
  thinkText: Style
}

export type ThemeScheme = 'dark' | 'light'
