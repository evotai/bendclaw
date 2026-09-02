/**
 * Prompt-cache display helpers (after pi's cache-stats).
 * Usage buckets are disjoint: uncached input + cache read + cache write.
 */

/**
 * Cache hit share of billed prompt tokens. One decimal in [99, 100) — a
 * steady loop always has a small uncached tail and rounding would pin the
 * display at a fake "100%"; "100" only when the whole prompt was read.
 */
export function formatCacheHitPercent(
  inputTokens: number,
  cacheReadTokens: number,
  cacheWriteTokens = 0,
): string {
  const total = inputTokens + cacheReadTokens + cacheWriteTokens
  if (total <= 0 || cacheReadTokens <= 0) return '0'
  if (cacheReadTokens >= total) return '100'
  const pct = (cacheReadTokens / total) * 100
  if (pct >= 99) return Math.min(99.9, Math.floor(pct * 10) / 10).toFixed(1)
  return String(Math.round(pct))
}
