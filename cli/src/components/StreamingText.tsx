/**
 * StreamingText component — renders the full streaming text as markdown.
 *
 * All streaming content stays in the dynamic zone until the response
 * completes, avoiding height-change jumps from frozen block transitions.
 */

import React, { useRef } from 'react'
import { Text, Box, useStdout } from 'ink'
import { renderMarkdown } from '../utils/markdown.js'

interface StreamingTextProps {
  text: string
  thinkingText: string
}

/** Only re-render markdown when text grows by a meaningful amount or hits a line boundary. */
const MD_RENDER_MIN_DELTA = 20

export const StreamingText = React.memo(function StreamingText({ text, thinkingText }: StreamingTextProps) {
  if (text.length === 0 && thinkingText.length === 0) {
    return null
  }

  const { stdout } = useStdout()
  // Reserve lines for: spinner(1) + input border(1) + input line(1) + footer(1) + border(1) + margin
  const maxLines = Math.max(5, (stdout?.rows ?? 24) - 8)

  const cacheRef = useRef({ text: '', rendered: '' })

  let rendered: string
  const cache = cacheRef.current
  const delta = text.length - cache.text.length
  const endsWithNewline = text.endsWith('\n')

  if (
    text === cache.text ||
    (delta > 0 && delta < MD_RENDER_MIN_DELTA && !endsWithNewline && cache.rendered.length > 0)
  ) {
    // Use cached render — text hasn't changed enough
    rendered = cache.rendered
  } else {
    rendered = text ? renderMarkdown(text) : ''
    cacheRef.current = { text, rendered }
  }

  // Truncate to last maxLines to keep spinner and input visible
  let displayText = rendered.replace(/^\n+/, '')
  const lines = displayText.split('\n')
  let truncated = false
  if (lines.length > maxLines) {
    displayText = lines.slice(-maxLines).join('\n')
    truncated = true
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
      {displayText.length > 0 && (
        <Box marginTop={1}>
          <Text color="magenta" bold>{'⏺ '}</Text>
          <Box flexDirection="column" flexShrink={1}>
            {truncated && <Text dimColor>{'  ···'}</Text>}
            <Text>{displayText}</Text>
          </Box>
        </Box>
      )}
    </Box>
  )
})
