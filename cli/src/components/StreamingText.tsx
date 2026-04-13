/**
 * StreamingText component — renders the current streaming assistant response.
 */

import React from 'react'
import { Text, Box } from 'ink'

interface StreamingTextProps {
  text: string
  thinkingText: string
}

export function StreamingText({ text, thinkingText }: StreamingTextProps) {
  if (text.length === 0 && thinkingText.length === 0) {
    return null
  }

  return (
    <Box flexDirection="column" marginBottom={1}>
      {thinkingText.length > 0 && (
        <Box marginBottom={0}>
          <Text dimColor italic>
            {thinkingText}
          </Text>
        </Box>
      )}
      {text.length > 0 && (
        <Box>
          <Text>{text}</Text>
          <Text color="gray">▍</Text>
        </Box>
      )}
    </Box>
  )
}
