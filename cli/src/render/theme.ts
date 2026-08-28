/**
 * Dark/light theme for terminal rendering.
 *
 * All ANSI-styled text goes through `theme.<field>.paint(s)` so a theme
 * swap is a single-point change. Colors are kept narrow (two brand hues
 * + three shades of gray) to stay coherent across components.
 */

import chalk, { type ChalkInstance } from 'chalk'

export interface Style {
  paint(text: string): string
}

const plain: Style = { paint: s => s }

function style(fn: (s: string) => string): Style {
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

  /** Fill behind a selected row (completion menu). */
  selectionBgHex: string
  /** Secondary text on `selectionBgHex`, where the normal dim gray is too dark. */
  selectionMutedHex: string
  /** The prompt caret. Deliberately off-palette so it reads as "you are here". */
  cursorHex: string
  /** Text sat on by the cursor block; contrasts against `cursorHex`. */
  cursorFgHex: string

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
  thinkBorder: Style
  thinkText: Style

  // Legacy aliases kept for existing call sites
  heading: string
  inlineCode: string
  linkColor: string
}

function darkTheme(): Theme {
  // Always call chalk.hex() at paint time. Binding `const accent = chalk.hex(...)`
  // at construction freezes chalk's color-level approximation: if the theme is
  // first built while chalk.level is 0/1 (no TTY, CI), headings/fences stay stuck
  // on 16-color SGR even after log-shot forces level 3. Lazy hex matches codeInline
  // / tableBorder and keeps truecolor stable across environments.
  // EVOT's primary periwinkle, sampled from the canonical terminal wordmark.
  const brandHex = '#b5bcf9'
  const accentHex = '#f0c674'
  return {
    brand: style(s => chalk.hex(brandHex)(s)),
    brandBold: style(s => chalk.hex(brandHex).bold(s)),
    brandHex,
    accent: style(s => chalk.hex(accentHex)(s)),
    accentBold: style(s => chalk.hex(accentHex).bold(s)),
    accentHex,

    // Desaturated periwinkle: reads as "same family as the frame" while staying
    // dark enough that brand-hued text on top keeps its contrast.
    selectionBgHex: '#2c2f4a',
    selectionMutedHex: '#9aa0b4',

    // Lime, against a periwinkle frame: the caret has to win attention over
    // everything around it, so it sits outside the brand palette. When the
    // cursor sits on a character, that cell flips to a lime block with near
    // black text — bright enough to read, close to a real terminal caret.
    cursorHex: '#9ae65c',
    cursorFgHex: '#1a1d24',

    text: plain,
    // Hue, not just weight: terminals whose font lacks a bold face drop SGR 1,
    // which made `**bold**` read as body text.
    bold: style(s => chalk.hex(accentHex).bold(s)),
    italic: style(s => chalk.italic(s)),
    boldItalic: style(s => chalk.hex(accentHex).bold.italic(s)),
    strikethrough: style(s => chalk.dim.strikethrough(s)),
    underline: style(s => chalk.underline(s)),
    // link style follows claudecode: rely on OSC 8 for clickability and keep
    // the URL in normal colour. Fallback is a bare URL without underline/hue.
    link: plain,
    // Inline code colour mirrors claudecode's `permission` hex exactly:
    // rgb(177,185,249) = #b1b9f9 (light blue-purple). Keeps `foo()`
    // references in the same semantic family as links without dominating
    // long prose on dark terminals.
    codeInline: style(s => chalk.hex('#b1b9f9')(s)),
    mathInline: style(s => chalk.hex('#8abeb7')(s)),
    mathBlock: style(s => chalk.hex('#8abeb7')(s)),

    // Headings carry evot's gold accent (matches the banner + pi's mdHeading).
    // h1 keeps the extra italic·underline emphasis; h2+ are accent-bold so
    // every level reads as a distinct section marker.
    h1: style(s => chalk.hex('#f0c674').bold.italic.underline(s)),
    h2: style(s => chalk.hex('#f0c674').bold(s)),
    h3: style(s => chalk.hex('#f0c674').bold(s)),
    h4: style(s => chalk.hex('#f0c674').bold(s)),
    h5: style(s => chalk.hex('#f0c674').bold(s)),
    h6: style(s => chalk.hex('#f0c674').bold(s)),

    // Secondary accent — the teal evot uses for banner links / 'evot update'.
    // pi tints list markers with its accent; we mirror that so bullets and
    // ordinals read as structure without competing with the gold headings.
    bullet: style(s => chalk.hex('#8abeb7')(s)),
    listNumber: style(s => chalk.hex('#8abeb7')(s)),

    blockquoteBorder: style(s => chalk.hex('#808080')(s)),
    // Italic but not dim — dimGray on dark backgrounds is nearly invisible
    // for long CJK quotes.
    blockquoteText: style(s => chalk.italic(s)),

    tableBorder: style(s => chalk.hex('#8a8a8a')(s)),
    tableHeader: style(s => chalk.bold(s)),

    hr: style(s => chalk.hex('#808080')(s)),
    codeBlockBorder: style(s => chalk.hex('#6a6a6a')(s)),
    thinkBorder: style(s => chalk.hex('#808080')(s)),
    thinkText: style(s => chalk.hex('#6a6a6a').italic(s)),

    heading: '#c0c0c0',
    inlineCode: '#5fb3b3',
    linkColor: 'blue',
  }
}

function lightTheme(): Theme {
  // Same lazy-hex rule as darkTheme — see comment there.
  const brandHex = '#5769f7'
  const accentHex = '#b8860b'
  return {
    brand: style(s => chalk.hex(brandHex)(s)),
    brandBold: style(s => chalk.hex(brandHex).bold(s)),
    brandHex,
    accent: style(s => chalk.hex(accentHex)(s)),
    accentBold: style(s => chalk.hex(accentHex).bold(s)),
    accentHex,

    // Light counterpart of the dark selection fill: a pale periwinkle wash that
    // keeps the brand-hued label readable on a white background.
    selectionBgHex: '#dfe3fd',
    selectionMutedHex: '#5b6070',

    // Darker, more saturated green: the dark theme's lime disappears on white.
    // The block flips to white text against it.
    cursorHex: '#3f9142',
    cursorFgHex: '#ffffff',

    text: plain,
    // See darkTheme: emphasis needs a hue, darker gold to hold contrast on white.
    bold: style(s => chalk.hex(accentHex).bold(s)),
    italic: style(s => chalk.italic(s)),
    boldItalic: style(s => chalk.hex(accentHex).bold.italic(s)),
    strikethrough: style(s => chalk.dim.strikethrough(s)),
    underline: style(s => chalk.underline(s)),
    // See darkTheme: link stays neutral and relies on OSC 8 for clickability.
    link: plain,
    // Inline code colour mirrors claudecode's `permission` hex exactly:
    // rgb(87,105,247) = #5769f7 (medium blue).
    codeInline: style(s => chalk.hex('#5769f7')(s)),
    mathInline: style(s => chalk.hex('#327878')(s)),
    mathBlock: style(s => chalk.hex('#327878')(s)),

    // Darker gold than the dark-theme accent so headings stay legible on a
    // light background (the #f0c674 gold washes out on white). Same warm
    // family as evot's brand accent.
    h1: style(s => chalk.hex('#b8860b').bold.italic.underline(s)),
    h2: style(s => chalk.hex('#b8860b').bold(s)),
    h3: style(s => chalk.hex('#b8860b').bold(s)),
    h4: style(s => chalk.hex('#b8860b').bold(s)),
    h5: style(s => chalk.hex('#b8860b').bold(s)),
    h6: style(s => chalk.hex('#b8860b').bold(s)),

    // See darkTheme: teal list markers. A slightly deeper teal reads better on
    // a light background than the dark-theme #8abeb7.
    bullet: style(s => chalk.hex('#5a8080')(s)),
    listNumber: style(s => chalk.hex('#5a8080')(s)),

    blockquoteBorder: style(s => chalk.hex('#6a6a6a')(s)),
    blockquoteText: style(s => chalk.italic(s)),

    tableBorder: style(s => chalk.hex('#8a8a8a')(s)),
    tableHeader: style(s => chalk.bold(s)),

    hr: style(s => chalk.hex('#6a6a6a')(s)),
    codeBlockBorder: style(s => chalk.hex('#8a8a8a')(s)),
    thinkBorder: style(s => chalk.hex('#6a6a6a')(s)),
    thinkText: style(s => chalk.hex('#8a8a8a').italic(s)),

    heading: '#333333',
    inlineCode: '#0d7d7d',
    linkColor: 'blue',
  }
}

export type ThemeScheme = 'dark' | 'light'

/** Runtime detection from OSC 11 / color-scheme DSR. Explicit EVOT_THEME always wins. */
let detectedScheme: ThemeScheme | null = null

function envThemeOverride(): ThemeScheme | null {
  const override = process.env.EVOT_THEME?.toLowerCase()
  if (override === 'light') return 'light'
  if (override === 'dark') return 'dark'
  return null
}

function detectSchemeFromEnv(): ThemeScheme {
  const colorfgbg = process.env.COLORFGBG
  if (colorfgbg) {
    const parts = colorfgbg.split(';')
    const bg = parseInt(parts[parts.length - 1] ?? '', 10)
    // High-numbered ANSI indices are typically light backgrounds.
    if (!isNaN(bg) && bg >= 8) return 'light'
  }
  return 'dark'
}

function resolveScheme(): ThemeScheme {
  return envThemeOverride() ?? detectedScheme ?? detectSchemeFromEnv()
}

let cached: Theme | null = null
let cachedScheme: ThemeScheme | null = null

export function getTheme(): Theme {
  const scheme = resolveScheme()
  if (cached && cachedScheme === scheme) return cached
  cachedScheme = scheme
  cached = scheme === 'dark' ? darkTheme() : lightTheme()
  return cached
}

export function getThemeScheme(): ThemeScheme {
  return resolveScheme()
}

/**
 * Apply a scheme detected from the terminal (OSC 11 / mode 2031).
 * Returns true when the effective theme changed (callers should rebuild
 * committed ANSI history). EVOT_THEME overrides make this a no-op.
 */
export function setDetectedThemeScheme(scheme: ThemeScheme): boolean {
  if (detectedScheme === scheme) return false
  detectedScheme = scheme
  if (envThemeOverride()) return false
  const previous = cachedScheme
  resetThemeCache()
  return previous !== resolveScheme()
}

/** Reset cached theme (for tests). */
export function resetThemeCache(): void {
  cached = null
  cachedScheme = null
}

/** Reset runtime detection + cache (for tests). */
export function resetDetectedThemeScheme(): void {
  detectedScheme = null
  resetThemeCache()
}

/** Exported for code that only needs the chalk instance. */
export function getChalk(): ChalkInstance {
  return chalk
}
