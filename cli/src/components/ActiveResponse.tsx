/**
 * ActiveResponse — minimal dynamic zone during loading.
 * Shows pending (incomplete) streaming text, tool progress, and Spinner.
 * pendingText holds the full streaming markdown for the current turn;
 * it is rendered with markdown formatting so it looks the same as the
 * final Static output.
 *
 * toolProgress holds the latest bash/tool output text, shown as tail
 * lines above the spinner (matching Rust REPL's dynamic refresh area).
 *
 * To keep the spinner and input box pinned at the bottom, we cap the
 * visible pending text to the last N lines that fit the terminal height.
 */

import React, { useMemo } from 'react'
import { Box, Text, useStdout } from 'ink'
import { ToolCallDisplay } from './ToolCallDisplay.js'
import { Spinner } from './Spinner.js'
import { renderMarkdown } from '../utils/markdown.js'
import type { UIToolCall } from '../state/AppState.js'

/** Lines reserved for spinner + prompt + padding */
const RESERVED_LINES = 6
/** Max progress tail lines shown above spinner */
const MAX_PROGRESS_LINES = 5
/** Max chars per progress line */
const MAX_PROGRESS_LINE_WIDTH = 120

interface Props {
  isLoading: boolean
  pendingText: string
  toolProgress: string
  activeToolCalls: Map<string, UIToolCall>
  outputTokens: number
  lastTokenAt: number
}

export function ActiveResponse({
  isLoading, pendingText, toolProgress, activeToolCalls, outputTokens, lastTokenAt,
}: Props) {
  if (!isLoading) return null

  const { stdout } = useStdout()
  const termRows = stdout?.rows ?? 24
  const maxLines = Math.max(termRows - RESERVED_LINES, 4)

  const hasTools = activeToolCalls.size > 0

  const rendered = useMemo(() => {
    if (!pendingText) return ''
    const full = renderMarkdown(pendingText).replace(/\n+$/, '')
    // Tail the output to fit the terminal
    const lines = full.split('\n')
    if (lines.length <= maxLines) return full
    return lines.slice(-maxLines).join('\n')
  }, [pendingText, maxLines])

  const progressLines = useMemo(() => {
    if (!toolProgress) return []
    const lines = toolProgress.split('\n')
    const tail = lines
      .slice(-MAX_PROGRESS_LINES)
      .map(l => l.length > MAX_PROGRESS_LINE_WIDTH
        ? l.slice(0, MAX_PROGRESS_LINE_WIDTH - 1) + '…'
        : l)
    // Pad to fixed height to prevent layout jumps
    while (tail.length < MAX_PROGRESS_LINES) {
      tail.unshift('')
    }
    return tail
  }, [toolProgress])

  return (
    <Box flexDirection="column">
      {rendered.length > 0 && (
        <Box>
          <Text>{'   '}{rendered}</Text>
        </Box>
      )}

      {hasTools && (
        <ToolCallDisplay tools={activeToolCalls} />
      )}

      {progressLines.length > 0 && (
        <Box flexDirection="column" marginTop={1}>
          {progressLines.map((line, i) => (
            <Box key={i} height={1}>
              <Text dimColor>{'  '}{line}</Text>
            </Box>
          ))}
        </Box>
      )}

      <Box marginTop={progressLines.length > 0 ? 0 : 1}>
        <Spinner
          toolName={hasTools ? [...activeToolCalls.values()][0]?.name : undefined}
          progressText={hasTools ? [...activeToolCalls.values()][0]?.previewCommand : undefined}
          tokenCount={outputTokens}
          lastTokenAt={lastTokenAt || undefined}
        />
      </Box>
    </Box>
  )
}
