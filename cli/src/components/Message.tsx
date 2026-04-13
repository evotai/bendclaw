/**
 * Message display component — renders a single message in the conversation.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { UIMessage } from '../state/AppState.js'

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
      {message.toolCalls?.map((tc) => (
        <Box key={tc.id} marginBottom={0}>
          <Text dimColor>
            {tc.status === 'error' ? '✗' : '✓'}{' '}
          </Text>
          <Text color={tc.status === 'error' ? 'red' : 'green'}>
            {tc.name}
          </Text>
          {tc.durationMs !== undefined && (
            <Text dimColor> ({tc.durationMs}ms)</Text>
          )}
        </Box>
      ))}
      {message.text.length > 0 && (
        <Box>
          <Text>{message.text}</Text>
        </Box>
      )}
    </Box>
  )
}
