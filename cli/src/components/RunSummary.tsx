/**
 * RunSummary — detailed stats after a run completes (verbose mode).
 * Matches the Rust REPL's "This Run Summary" format with bars and breakdowns.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { RunStats } from '../state/AppState.js'
import { humanTokens, formatDuration, renderBar } from '../utils/format.js'

const TOP_LLM_CALLS = 3

interface RunSummaryProps {
  stats: RunStats
}

export function RunSummary({ stats }: RunSummaryProps) {
  const totalTokens = stats.inputTokens + stats.outputTokens
  const llmTotal = llmTotalMs(stats)
  const avgTokPerSec = llmTotal > 0 ? (stats.outputTokens / (llmTotal / 1000)).toFixed(1) : '—'

  // Context budget — use real system_prompt_tokens when available
  const sysTok = stats.systemPromptTokens > 0 ? stats.systemPromptTokens : 4000
  const budget = stats.contextWindow > 0 ? stats.contextWindow - sysTok : 0
  const ctxPct = budget > 0 ? ((stats.contextTokens / budget) * 100).toFixed(0) : '0'
  const ctxBar = renderBar(stats.contextTokens, budget, 20)

  // Token breakdown by role — estimate from tool breakdown
  const totalToolResultTokens = stats.toolBreakdown.reduce((s, t) => s + t.totalResultTokens, 0)

  // Cache hit rate
  const totalCacheTokens = stats.cacheReadTokens + stats.cacheWriteTokens
  const cacheHitRate = totalCacheTokens > 0
    ? ((stats.cacheReadTokens / stats.inputTokens) * 100).toFixed(1)
    : null

  // LLM calls: top N + "N more"
  const sortedLlmCalls = [...stats.llmCallDetails].sort((a, b) => b.durationMs - a.durationMs)
  const topCalls = sortedLlmCalls.slice(0, TOP_LLM_CALLS)
  const restCalls = sortedLlmCalls.slice(TOP_LLM_CALLS)
  const restTotalMs = restCalls.reduce((s, c) => s + c.durationMs, 0)

  // Tool breakdown sorted by result tokens (descending), fallback to count
  const sortedTools = [...stats.toolBreakdown].sort((a, b) =>
    (b.totalResultTokens || b.count) - (a.totalResultTokens || a.count)
  )

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text dimColor>─── This Run Summary ──────────────────────────────────</Text>

      {/* Top line */}
      <Text dimColor>
        {formatDuration(stats.durationMs)} · {stats.turnCount} turns · {stats.llmCalls} llm calls · {stats.toolCallCount} tool calls · {humanTokens(totalTokens)} tokens
      </Text>

      {/* Context budget */}
      {budget > 0 && (
        <Text dimColor>
          {'  context   '}{ctxBar}  {ctxPct}%({humanTokens(stats.contextTokens)}) of budget({humanTokens(budget)})
        </Text>
      )}

      {/* Token breakdown */}
      <Text dimColor>{''}</Text>
      <Text dimColor>
        {'  tokens    '}{humanTokens(stats.inputTokens)} total input · {humanTokens(stats.outputTokens)} output · {avgTokPerSec} tok/s
      </Text>

      {/* Per-role token estimates */}
      {stats.inputTokens > 0 && (
        <>
          <Text dimColor>
            {'            system          ~'}{humanTokens(sysTok)}  {renderBar(sysTok, stats.inputTokens, 20)}  {pct(sysTok, stats.inputTokens)}
          </Text>
          {totalToolResultTokens > 0 && (
            <Text dimColor>
              {'            tool_result    ~'}{humanTokens(totalToolResultTokens)}  {renderBar(totalToolResultTokens, stats.inputTokens, 20)}  {pct(totalToolResultTokens, stats.inputTokens)}
            </Text>
          )}
          {totalToolResultTokens > 0 && sortedTools.filter((t) => t.totalResultTokens > 0).map((tc, i) => (
            <Text key={i} dimColor>
              {'              '}{padName(tc.name, 16)}{tc.count} calls  ~{humanTokens(tc.totalResultTokens).padStart(5)}  {renderBar(tc.totalResultTokens, totalToolResultTokens, 17)} {pct(tc.totalResultTokens, totalToolResultTokens)}
            </Text>
          ))}
        </>
      )}

      {/* Cache info */}
      {(stats.cacheReadTokens > 0 || stats.cacheWriteTokens > 0) && (
        <Text dimColor>
          {'            cache read '}{humanTokens(stats.cacheReadTokens)} · cache write {humanTokens(stats.cacheWriteTokens)}{cacheHitRate ? ` · hit rate ${cacheHitRate}%` : ''}
        </Text>
      )}

      {/* Compaction history */}
      {stats.compactionHistory.length > 0 && (
        <>
          <Text dimColor>{''}</Text>
          <Text dimColor>
            {'  compact   '}{stats.compactionHistory.length} compaction{stats.compactionHistory.length > 1 ? 's' : ''} · saved {humanTokens(totalCompactionSaved(stats))} tokens
          </Text>
          {stats.compactionHistory.map((c, i) => {
            const label = c.kind === 'run_once' ? 'run-once' : `#${i + 1}  lv${c.level}`
            const savedPct = c.beforeTokens > 0 ? ((c.savedTokens / c.beforeTokens) * 100).toFixed(0) : '0'
            return (
              <Text key={i} dimColor>
                {'            '}{label}  {humanTokens(c.beforeTokens)}→{humanTokens(c.afterTokens)}  saved {humanTokens(c.savedTokens)}  {renderBar(c.savedTokens, c.beforeTokens, 20)} {savedPct}%
              </Text>
            )
          })}
        </>
      )}

      {/* LLM call breakdown */}
      {stats.llmCallDetails.length > 0 && (
        <>
          <Text dimColor>{''}</Text>
          <Text dimColor>
            {'  llm       '}{stats.llmCalls} calls · {formatDuration(llmTotal)} ({pct(llmTotal, stats.durationMs)} of run) · {avgTokPerSec} tok/s avg
          </Text>
          {stats.llmCallDetails.length > 1 && (
            <Text dimColor>
              {'            ttft avg '}{formatDuration(avgTtft(stats))} · stream avg {formatDuration(avgStream(stats))}
            </Text>
          )}
          {topCalls.map((call) => {
            const origIdx = stats.llmCallDetails.indexOf(call)
            return (
              <Text key={origIdx} dimColor>
                {'            #'}{origIdx + 1}    {formatDuration(call.durationMs).padStart(5)} {renderBar(call.durationMs, stats.durationMs, 20)} {pct(call.durationMs, stats.durationMs)}
              </Text>
            )
          })}
          {restCalls.length > 0 && (
            <Text dimColor>
              {'            ... '}{restCalls.length} more call{restCalls.length > 1 ? 's' : ''} · {formatDuration(restTotalMs)} total
            </Text>
          )}
        </>
      )}

      <Text dimColor>────────────────────────────────────────────────────────</Text>
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function pct(value: number, total: number): string {
  if (total <= 0) return '0%'
  return `${((value / total) * 100).toFixed(1)}%`
}

function llmTotalMs(stats: RunStats): number {
  return stats.llmCallDetails.reduce((sum, c) => sum + c.durationMs, 0)
}

function avgTtft(stats: RunStats): number {
  if (stats.llmCallDetails.length === 0) return 0
  return stats.llmCallDetails.reduce((sum, c) => sum + c.ttftMs, 0) / stats.llmCallDetails.length
}

function avgStream(stats: RunStats): number {
  if (stats.llmCallDetails.length === 0) return 0
  return stats.llmCallDetails.reduce((sum, c) => sum + c.streamingMs, 0) / stats.llmCallDetails.length
}

function totalCompactionSaved(stats: RunStats): number {
  return stats.compactionHistory.reduce((sum, c) => sum + c.savedTokens, 0)
}

function padName(name: string, width: number): string {
  if (name.length >= width) return name + ' '
  return name + ' '.repeat(width - name.length)
}
