/**
 * StreamingText component — renders the current streaming assistant response.
 * Applies markdown rendering during streaming for code blocks, tables, etc.
 */

import React from 'react'
import { Text, Box } from 'ink'
import { renderStreamingText, shouldRefreshStreamingMarkdown, splitStreamingMarkdown } from '../utils/streaming.js'

const STREAMING_MARKDOWN_INTERVAL_MS = 120

interface StreamingTextProps {
  text: string
  thinkingText: string
}

export function StreamingText({ text, thinkingText }: StreamingTextProps) {
  const [rendered, setRendered] = React.useState(() => (text.length > 0 ? renderStreamingText(text) : ''))
  const lastRenderedAtRef = React.useRef(0)
  const stablePrefixRef = React.useRef('')
  const stableRenderedRef = React.useRef('')

  React.useEffect(() => {
    if (text.length === 0) {
      setRendered('')
      lastRenderedAtRef.current = 0
      stablePrefixRef.current = ''
      stableRenderedRef.current = ''
      return
    }

    const now = Date.now()
    const update = () => {
      const { stablePrefix, unstableSuffix } = splitStreamingMarkdown(text, stablePrefixRef.current)
      if (stablePrefix !== stablePrefixRef.current) {
        stablePrefixRef.current = stablePrefix
        stableRenderedRef.current = stablePrefix.length > 0 ? renderStreamingText(stablePrefix) : ''
      }
      lastRenderedAtRef.current = Date.now()
      const unstableRendered = unstableSuffix.length > 0 ? renderStreamingText(unstableSuffix) : ''
      setRendered(stableRenderedRef.current + unstableRendered)
    }

    if (shouldRefreshStreamingMarkdown(lastRenderedAtRef.current, now, STREAMING_MARKDOWN_INTERVAL_MS)) {
      update()
      return
    }

    const delay = STREAMING_MARKDOWN_INTERVAL_MS - (now - lastRenderedAtRef.current)
    const timer = setTimeout(update, delay)
    return () => clearTimeout(timer)
  }, [text])

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
