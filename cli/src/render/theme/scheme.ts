/**
 * Scheme resolution and the cached active theme.
 *
 * Priority: explicit `EVOT_THEME` → scheme detected at runtime from the
 * terminal (OSC 11 / color-scheme DSR) → `COLORFGBG` heuristic → dark.
 */

import chalk, { type ChalkInstance } from 'chalk'
import { darkTheme } from './dark.js'
import { lightTheme } from './light.js'
import type { Theme, ThemeScheme } from './types.js'

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
