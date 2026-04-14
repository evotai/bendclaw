/**
 * ToolCallDisplay — shows active and completed tool calls during streaming.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { UIToolCall } from '../state/AppState.js'

interface ToolCallDisplayProps {
  tools: Map<string, UIToolCall>
}

export function ToolCallDisplay({ tools }: ToolCallDisplayProps) {
  const entries = [...tools.values()]
  if (entries.length === 0) return null

  return (
    <Box flexDirection="column" marginBottom={0}>
      {entries.map((tool) => (
        <ToolCallLine key={tool.id} tool={tool} />
      ))}
    </Box>
  )
}

function ToolCallLine({ tool }: { tool: UIToolCall }) {
  const icon = statusIcon(tool.status)
  const color = statusColor(tool.status)
  const detail = formatToolDetail(tool)

  return (
    <Box>
      <Text color={color}>{icon} </Text>
      <Text color={color} bold>{tool.name}</Text>
      {detail && <Text dimColor> {detail}</Text>}
      {tool.status === 'done' && tool.durationMs !== undefined && (
        <Text dimColor> ({tool.durationMs}ms)</Text>
      )}
    </Box>
  )
}

function statusIcon(status: UIToolCall['status']): string {
  switch (status) {
    case 'running': return '⟡'
    case 'done': return '✓'
    case 'error': return '✗'
  }
}

function statusColor(status: UIToolCall['status']): string {
  switch (status) {
    case 'running': return 'yellow'
    case 'done': return 'green'
    case 'error': return 'red'
  }
}

function formatToolDetail(tool: UIToolCall): string {
  if (tool.previewCommand) return tool.previewCommand

  const args = tool.args
  if (!args || typeof args !== 'object') return ''

  // Show a compact summary of common tool args
  if ('command' in args) return truncate(String(args.command), 80)
  if ('path' in args) return truncate(String(args.path), 80)
  if ('file_path' in args) return truncate(String(args.file_path), 80)
  if ('pattern' in args) return truncate(String(args.pattern), 60)
  if ('url' in args) return truncate(String(args.url), 80)
  if ('question' in args) return truncate(String(args.question), 60)

  return ''
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s
  return s.slice(0, max - 1) + '…'
}
