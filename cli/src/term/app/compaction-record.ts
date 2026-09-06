import type { CompactRecord } from './types.js'
import { toolArgsRecord } from './tool-args.js'

/**
 * Project a `context_compaction_completed` result into the run's compact
 * history. The current Rust writer emits `compacted` / `no_op`; the older
 * level-based shapes are still read so historical verbose fixtures keep the
 * same summary. Field access is by narrowing, never by cast.
 */
export function compactRecordFromResult(result: unknown, currentContextTokens: number): CompactRecord | null {
  const fields = toolArgsRecord(result)
  const type = typeof fields?.type === 'string' ? fields.type : 'done'
  if (!fields) return null
  const first = (...names: string[]): number | undefined => {
    for (const name of names) {
      const value = fields[name]
      if (typeof value === 'number' && Number.isFinite(value)) return value
    }
    return undefined
  }
  switch (type) {
    case 'compacted':
    case 'level_compacted':
    case 'level_done':
      return {
        level: first('level') ?? (first('messages_evicted') ? 3 : 1),
        beforeTokens: first('before_estimated_tokens', 'before_tokens', 'tokens_before') ?? 0,
        afterTokens: first('after_estimated_tokens', 'after_tokens', 'tokens_after') ?? 0,
      }
    case 'run_once_cleared':
      return {
        level: 0,
        beforeTokens: first('before_estimated_tokens') ?? currentContextTokens,
        afterTokens: first('after_estimated_tokens') ?? (currentContextTokens - (first('saved_tokens') ?? 0)),
      }
    default:
      return null
  }
}
