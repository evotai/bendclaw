/**
 * Message display component — renders a single message in the conversation.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { UIMessage, UIToolCall } from '../state/AppState.js'

interface MessageProps {
  message: UIMessage
}

export function Message({ message }: MessageProps) {
  if (message.role === 'user') {
    return <UserMessage message={message} />
  }
  return <AssistantMessage message={message} />
}

function UserMessage({ message }: MessageProps) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box>
        <Text bold color="yellow">{'> '}</Text>
        <Text bold>{message.text}</Text>
      </Box>
    </Box>
  )
}

function AssistantMessage({ message }: MessageProps) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      {/* Tool call results */}
      {message.toolCalls?.map((tc) => (
        <ToolCallResult key={tc.id} tool={tc} />
      ))}

      {/* Assistant text */}
      {message.text.length > 0 && (
        <Box>
          <Text>{message.text}</Text>
        </Box>
      )}
    </Box>
  )
}

function ToolCallResult({ tool }: { tool: UIToolCall }) {
  const icon = tool.status === 'error' ? '✗' : '✓'
  const color = tool.status === 'error' ? 'red' : 'green'
  const detail = formatToolSummary(tool)

  return (
    <Box>
      <Text color={color}>{icon} </Text>
      <Text color={color}>{tool.name}</Text>
      {detail && <Text dimColor> {detail}</Text>}
      {tool.durationMs !== undefined && (
        <Text dimColor> ({tool.durationMs}ms)</Text>
      )}
    </Box>
  )
}

function formatToolSummary(tool: UIToolCall): string {
  // Show a compact summary of what the tool did
  if (tool.result && tool.status === 'error') {
    return truncate(tool.result, 100)
  }

  const args = tool.args
  if (!args || typeof args !== 'object') return ''

  if ('command' in args) return truncate(String(args.command), 80)
  if ('path' in args) return truncate(String(args.path), 80)
  if ('file_path' in args) return truncate(String(args.file_path), 80)
  if ('pattern' in args) return truncate(String(args.pattern), 60)
  if ('url' in args) return truncate(String(args.url), 80)

  return ''
}

function truncate(s: string, max: number): string {
  // Collapse to single line and truncate
  const oneLine = s.replace(/\n/g, ' ').trim()
  if (oneLine.length <= max) return oneLine
  return oneLine.slice(0, max - 1) + '…'
}
