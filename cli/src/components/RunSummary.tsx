/**
 * RunSummary — detailed stats after a run completes (verbose mode).
 * Matches the Rust REPL's "This Run Summary" format with bars and breakdowns.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { RunStats } from '../state/AppState.js'

interface RunSummaryProps {
  stats: RunStats
}

export function RunSummary({ stats }: RunSummaryProps) {
  const totalTokens = stats.inputTokens + stats.outputTokens
  const durationSec = stats.durationMs / 1000
  const tokPerSec = durationSec > 0 ? (totalTokens / durationSec).toFixed(1) : '—'

  // Context budget
  const budget = stats.contextWindow > 0 ? stats.contextWindow - 4000 : 0 // approx sys prompt
  const ctxPct = budget > 0 ? ((stats.contextTokens / budget) * 100).toFixed(0) : '0'
  const ctxBar = renderBar(stats.contextTokens, budget, 20)

  // Token category estimates (approximate from typical ratios)
  const sysTokens = Math.round(stats.inputTokens * 0.2)
  const userTokens = Math.round(stats.inputTokens * 0.1)
  const assistantTokens = stats.inputTokens - sysTokens - userTokens

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text dimColor>─── This Run Summary ──────────────────────────────────</Text>

      {/* Top line */}
      <Text dimColor>
        {fmtDur(stats.durationMs)} · {stats.turnCount} turns · {stats.llmCalls} llm calls · {stats.toolCallCount} tool calls · {humanTok(totalTokens)} tokens
      </Text>

      {/* Context budget */}
      {budget > 0 && (
        <>
          <Text dimColor>
            {'  context   '}{ctxBar}  {ctxPct}%({humanTok(stats.contextTokens)}) of budget({humanTok(budget)})
          </Text>
        </>
      )}

      {/* Token breakdown */}
      <Text dimColor>{''}</Text>
      <Text dimColor>
        {'  tokens    '}{humanTok(stats.inputTokens)} total input · {stats.outputTokens} output · {tokPerSec} tok/s
      </Text>
      <Text dimColor>
        {'            system             '}{humanTok(sysTokens)}  {renderBar(sysTokens, stats.inputTokens, 20)}  {pct(sysTokens, stats.inputTokens)}
      </Text>
      <Text dimColor>
        {'            user               '}{humanTok(userTokens)}  {renderBar(userTokens, stats.inputTokens, 20)}  {pct(userTokens, stats.inputTokens)}
      </Text>
      <Text dimColor>
        {'            assistant           '}{humanTok(assistantTokens)}  {renderBar(assistantTokens, stats.inputTokens, 20)}  {pct(assistantTokens, stats.inputTokens)}
      </Text>

      {stats.cacheReadTokens > 0 && (
        <Text dimColor>
          {'            cache read '}{humanTok(stats.cacheReadTokens)} · cache write {humanTok(stats.cacheWriteTokens)}
        </Text>
      )}

      {/* LLM call breakdown */}
      {stats.llmCallDetails.length > 0 && (
        <>
          <Text dimColor>{''}</Text>
          <Text dimColor>
            {'  llm       '}{stats.llmCalls} calls · {fmtDur(stats.durationMs)} ({pct(llmTotalMs(stats), stats.durationMs)} of run) · {tokPerSec} tok/s avg
          </Text>
          {stats.llmCallDetails.length > 1 && (
            <Text dimColor>
              {'            ttft avg '}{fmtDur(avgTtft(stats))} · stream avg {fmtDur(avgStream(stats))}
            </Text>
          )}
          {stats.llmCallDetails.map((call, i) => (
            <Text key={i} dimColor>
              {'            #'}{i + 1}  {fmtDur(call.durationMs)} {renderBar(call.durationMs, stats.durationMs, 20)} {pct(call.durationMs, stats.durationMs)}
            </Text>
          ))}
        </>
      )}

      {/* Tool breakdown */}
      {stats.toolBreakdown.length > 0 && (
        <>
          <Text dimColor>{''}</Text>
          {stats.toolBreakdown
            .sort((a, b) => b.count - a.count)
            .map((tc, i) => (
              <Text key={i} dimColor>
                {'              '}{tc.name}  {tc.count} call{tc.count > 1 ? 's' : ''}  {fmtDur(tc.totalDurationMs)}
                {tc.errors > 0 ? `  (${tc.errors} error${tc.errors > 1 ? 's' : ''})` : ''}
              </Text>
            ))}
        </>
      )}

      <Text dimColor>────────────────────────────────────────────────────────</Text>
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function humanTok(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`
  return `${n}`
}

function fmtDur(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function pct(value: number, total: number): string {
  if (total <= 0) return '0%'
  return `${((value / total) * 100).toFixed(1)}%`
}

function renderBar(value: number, max: number, width: number): string {
  if (max <= 0) return '░'.repeat(width)
  const filled = Math.round((value / max) * width)
  return '█'.repeat(Math.min(filled, width)) + '░'.repeat(Math.max(0, width - filled))
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
  return stats.llmCallDetails.reduce((sum, c) => sum + (c.durationMs - c.ttftMs), 0) / stats.llmCallDetails.length
}