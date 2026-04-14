/**
 * StreamingText component — renders the current streaming assistant response.
 * Applies markdown rendering during streaming for code blocks, tables, etc.
 */

import React from 'react'
import { Text, Box } from 'ink'
import { renderStreamingText } from '../utils/streaming.js'

interface StreamingTextProps {
  text: string
  thinkingText: string
}

export function StreamingText({ text, thinkingText }: StreamingTextProps) {
  if (text.length === 0 && thinkingText.length === 0) {
    return null
  }

  const rendered = text.length > 0 ? renderStreamingText(text) : ''

  return (
    <Box flexDirection="column" marginBottom={1}>
      {thinkingText.length > 0 && (
        <Box marginBottom={0}>
          <Text dimColor italic>
            {thinkingText}
          </Text>
        </Box>
      )}
      {rendered.length > 0 && (
        <Box marginTop={1}>
          <Text color="magenta" bold>{'⏺ '}</Text>
          <Box flexDirection="column" flexShrink={1}>
            <Text>{rendered.replace(/^\n+/, '')}</Text>
            <Text color="gray">▍</Text>
          </Box>
        </Box>
      )}
    </Box>
  )
}
