/**
 * Streaming utilities for block-level append rendering.
 *
 * Splits streaming text at top-level markdown block boundaries.
 * Completed blocks are frozen (appended to <Static>), only the last
 * growing block stays in the dynamic zone.
 *
 * marked.lexer() treats unclosed code fences as a single token,
 * so block boundaries are always safe split points.
 */

import { marked } from 'marked'

export interface StableBlockSplit {
  /** Individual completed block raw texts, ready to be frozen */
  stableTexts: string[]
  /** New absolute boundary position in the full text */
  newBoundary: number
}

/**
 * Split text from `boundary` onward into completed blocks + a growing tail.
 *
 * Returns the raw text of each completed block individually (so they can
 * be rendered and frozen one by one), plus the new boundary position.
 */
export function splitStableBlocks(text: string, boundary: number): StableBlockSplit {
  const tail = text.substring(boundary)
  if (!tail) return { stableTexts: [], newBoundary: boundary }

  let tokens: ReturnType<typeof marked.lexer>
  try {
    tokens = marked.lexer(tail)
  } catch {
    return { stableTexts: [], newBoundary: boundary }
  }

  if (tokens.length <= 1) return { stableTexts: [], newBoundary: boundary }

  // Find the last non-space token — that's the growing block
  let lastContentIdx = tokens.length - 1
  while (lastContentIdx >= 0 && tokens[lastContentIdx]!.type === 'space') {
    lastContentIdx--
  }

  if (lastContentIdx <= 0) return { stableTexts: [], newBoundary: boundary }

  // Collect each completed block's raw text
  const stableTexts: string[] = []
  let advance = 0
  for (let i = 0; i < lastContentIdx; i++) {
    const raw = tokens[i]!.raw
    // Skip pure whitespace/space tokens — they're just separators
    if (tokens[i]!.type !== 'space') {
      stableTexts.push(raw)
    }
    advance += raw.length
  }

  return {
    stableTexts,
    newBoundary: boundary + advance,
  }
}

/**
 * @deprecated Use splitStableBlocks instead. Kept for test compatibility.
 */
export function findStableBoundary(text: string, boundary: number): number {
  const { newBoundary } = splitStableBlocks(text, boundary)
  return newBoundary - boundary
}

export function renderStreamingText(text: string): string {
  return text
}

export function shouldAnimateTerminalTitle(): boolean {
  return process.env.EVOT_ANIMATE_TITLE === '1'
}
