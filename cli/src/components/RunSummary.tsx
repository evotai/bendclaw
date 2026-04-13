/**
 * RunSummary component — displays detailed stats after a run completes.
 * Only shown when verbose mode is enabled (Ctrl+L toggle).
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { RunStats } from '../state/AppState.js'

interface RunSummaryProps {
  stats: RunStats
}

export function RunSummary({ stats }: RunSummaryProps) {
  const duration = formatDuration(stats.durationMs)
  const turns = stats.turnCount
  const toolCalls = stats.toolCallCount
  const toolErrors = stats.toolErrorCount

  return (
    <Box flexDirection="column" marginBottom={1} marginTop={0}>
      <Box>
        <Text dimColor>{'─── Run Summary '}</Text>
        <Text dimColor>{'─'.repeat(40)}</Text>
      </Box>

      <Box>
        <Text dimColor>
          {duration} · {turns} turn{turns !== 1 ? 's' : ''} · {toolCalls} tool call{toolCalls !== 1 ? 's' : ''}
          {toolErrors > 0 ? ` · ${toolErrors} error${toolErrors !== 1 ? 's' : ''}` : ''}
        </Text>
      </Box>

      {/* Token usage */}
      {(stats.inputTokens > 0 || stats.outputTokens > 0) && (
        <Box flexDirection="column">
          <Box>
            <Text dimColor>
              tokens  {formatTokens(stats.inputTokens)} in · {formatTokens(stats.outputTokens)} out
              {stats.cacheReadTokens > 0 ? ` · ${formatTokens(stats.cacheReadTokens)} cache` : ''}
            </Text>
          </Box>
        </Box>
      )}

      {/* LLM calls breakdown */}
      {stats.llmCalls > 0 && (
        <Box>
          <Text dimColor>
            llm     {stats.llmCalls} call{stats.llmCalls !== 1 ? 's' : ''} · {duration}
          </Text>
        </Box>
      )}

      {/* Tool breakdown */}
      {stats.toolBreakdown.length > 0 && (
        <Box flexDirection="column">
          {stats.toolBreakdown.slice(0, 5).map((tb) => (
            <Box key={tb.name}>
              <Text dimColor>
                {'  '}{padRight(tb.name, 16)} {tb.count} call{tb.count !== 1 ? 's' : ''}
                {tb.totalDurationMs > 0 ? `  ${formatDuration(tb.totalDurationMs)}` : ''}
                {tb.errors > 0 ? `  ${tb.errors} err` : ''}
              </Text>
            </Box>
          ))}
        </Box>
      )}

      {/* Context info */}
      {stats.contextTokens > 0 && (
        <Box>
          <Text dimColor>
            context {formatTokens(stats.contextTokens)} tokens
            {stats.contextWindow > 0 ? ` · ${Math.round(stats.contextTokens / stats.contextWindow * 100)}% of ${formatTokens(stats.contextWindow)}` : ''}
          </Text>
        </Box>
      )}
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  const secs = ms / 1000
  if (secs < 60) return `${secs.toFixed(1)}s`
  const mins = Math.floor(secs / 60)
  const remainSecs = secs % 60
  return `${mins}m${remainSecs.toFixed(0)}s`
}

function formatTokens(n: number): string {
  if (n < 1000) return String(n)
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`
  return `${(n / 1_000_000).toFixed(2)}M`
}

function padRight(s: string, n: number): string {
  return s + ' '.repeat(Math.max(0, n - s.length))
}
