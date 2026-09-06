import type { ManualCompactionOutcome } from '../../native/contracts/results.js'
import { buildAssistantLines, type OutputLine } from '../../render/output.js'
import { formatCompactionCompleted } from '../../render/verbose.js'

type Compacted = Extract<ManualCompactionOutcome, { status: 'compacted' }>

/** Pure presentation of a manual compaction; independent of terminal/history
 * ownership, session loading, timers and native task lifecycle. */
export function manualCompactionLines(outcome: Compacted): { compact: OutputLine[]; expanded: OutputLine[] } {
  const details = formatCompactionCompleted({
    reason: 'manual', context_window: outcome.context_window,
    result: {
      type: 'compacted', before_message_count: outcome.messages_before,
      after_message_count: outcome.messages_after, before_tokens: outcome.tokens_before,
      after_tokens: outcome.tokens_after, messages_evicted: outcome.messages_evicted,
      current_run_reclaimed: outcome.current_run_reclaimed, compaction_level: outcome.compaction_level,
      method: outcome.method ?? 'local', remote_blob_bytes: outcome.remote_blob_bytes,
      fallback_reason: outcome.fallback_reason,
    },
  })
  const headline = details.split('\n')[0]?.replace(/^\[COMPACT\] ✓ · /, '') ?? 'manual'
  const label: OutputLine = { id: 'sys-compact-label', kind: 'system', text: '  [compaction]' }
  const status: OutputLine = { id: 'sys-compact-result', kind: 'system', text: `  Compacted · ${headline} (ctrl+o to expand)` }
  const compact = [label, status]
  const expanded = [label, { ...status, text: `  ${details}` }, ...buildAssistantLines(outcome.summary, { idPrefix: 'sys-compact-summary' })]
  const both = (line: OutputLine) => { compact.push(line); expanded.push(line) }
  if (outcome.used_fallback) both({
    id: 'sys-compact-fallback', kind: 'system',
    text: '  Note: the LLM summary was unavailable; a deterministic fallback summary was used.',
  })
  if (outcome.context_window > 0 && outcome.tokens_after >= outcome.context_window) both({
    id: 'sys-compact-warning', kind: 'error',
    text: `Context is still ${outcome.tokens_after.toLocaleString()} tokens, above this model's ${outcome.context_window.toLocaleString()}-token window. Switch to a larger-context model or start a new session.`,
  })
  return { compact, expanded }
}
