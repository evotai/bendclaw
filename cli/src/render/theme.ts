/**
 * Minimal dark/light theme for terminal rendering.
 *
 * Detects background brightness from COLORFGBG (e.g. "15;0" → dark)
 * and falls back to dark. Override with EVOT_THEME=light|dark.
 */

export interface Theme {
  // Markdown
  heading: string
  inlineCode: string
  // Diff
  addedBg: [number, number, number]
  removedBg: [number, number, number]
  addedWord: [number, number, number]
  removedWord: [number, number, number]
  // Links
  linkColor: string
}

const dark: Theme = {
  heading: '#c0c0c0',
  inlineCode: '#5fb3b3',
  addedBg: [2, 40, 0],
  removedBg: [61, 1, 0],
  addedWord: [4, 71, 0],
  removedWord: [92, 2, 0],
  linkColor: 'blue',
}

const light: Theme = {
  heading: '#333333',
  inlineCode: '#0d7d7d',
  addedBg: [210, 255, 210],
  removedBg: [255, 220, 220],
  addedWord: [170, 235, 170],
  removedWord: [255, 185, 185],
  linkColor: 'blue',
}

function detectDarkBackground(): boolean {
  const env = process.env
  // Explicit override
  const override = env.EVOT_THEME?.toLowerCase()
  if (override === 'light') return false
  if (override === 'dark') return true
  // COLORFGBG is "fg;bg" — bg >= 8 usually means light background
  const colorfgbg = env.COLORFGBG
  if (colorfgbg) {
    const parts = colorfgbg.split(';')
    const bg = parseInt(parts[parts.length - 1] ?? '', 10)
    if (!isNaN(bg) && bg >= 8) return false
  }
  // Default to dark
  return true
}

let cached: Theme | null = null

export function getTheme(): Theme {
  if (cached) return cached
  cached = detectDarkBackground() ? dark : light
  return cached
}

/** Reset cached theme (for tests). */
export function resetThemeCache(): void {
  cached = null
}
