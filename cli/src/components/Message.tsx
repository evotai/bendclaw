/**
 * Message display component — renders a single message in the conversation.
 * In verbose mode, shows additional detail for tool calls.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { UIMessage, UIToolCall } from '../state/AppState.js'

interface MessageProps {
  message: UIMessage
  verbose?: boolean
}

export function Message({ message, verbose = false }: MessageProps) {
  if (message.role === 'user') {
    return <UserMessage message={message} />
  }
  return <AssistantMessage message={message} verbose={verbose} />
}

function UserMessage({ message }: { message: UIMessage }) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box>
        <Text bold color="yellow">{'❯ '}</Text>
        <Text bold>{message.text}</Text>
      </Box>
    </Box>
  )
}

function AssistantMessage({ message, verbose }: { message: UIMessage; verbose: boolean }) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      {/* Tool call results */}
      {message.toolCalls?.map((tc) => (
        <ToolCallResult key={tc.id} tool={tc} verbose={verbose} />
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

function ToolCallResult({ tool, verbose }: { tool: UIToolCall; verbose: boolean }) {
  const icon = tool.status === 'error' ? '✗' : '✓'
  const color = tool.status === 'error' ? 'red' : 'green'
  const detail = formatToolSummary(tool)

  return (
    <Box flexDirection="column">
      <Box>
        <Text color={color}>{icon} </Text>
        <Text color={color} bold>{tool.name}</Text>
        {detail && <Text dimColor> {detail}</Text>}
        {tool.durationMs !== undefined && (
          <Text dimColor> ({tool.durationMs}ms)</Text>
        )}
      </Box>

      {/* Verbose: show tool result preview */}
      {verbose && tool.result && (
        <Box marginLeft={2} marginBottom={0}>
          <Text dimColor>
            {truncateResult(tool.result, tool.status === 'error' ? 200 : 120)}
          </Text>
        </Box>
      )}
    </Box>
  )
}

function formatToolSummary(tool: UIToolCall): string {
  if (tool.result && tool.status === 'error') {
    return truncate(tool.result.split('\n')[0] ?? '', 100)
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
  const oneLine = s.replace(/\n/g, ' ').trim()
  if (oneLine.length <= max) return oneLine
  return oneLine.slice(0, max - 1) + '…'
}

function truncateResult(s: string, maxChars: number): string {
  const lines = s.split('\n')
  let result = ''
  for (const line of lines) {
    if (result.length + line.length > maxChars) {
      result += '…'
      break
    }
    if (result.length > 0) result += '\n'
    result += line
  }
  return result
}
