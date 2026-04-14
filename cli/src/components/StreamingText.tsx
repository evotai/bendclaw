/**
 * StreamingText component — renders the current streaming assistant response.
 *
 * Uses block-level splitting: completed markdown blocks are rendered in a
 * memoized sub-component that won't trigger ink redraws when unchanged.
 * Only the last growing block re-renders per delta.
 */

import React, { useRef, memo } from 'react'
import { Text, Box } from 'ink'
import { renderMarkdown } from '../utils/markdown.js'
import { splitStableBlocks } from '../utils/streaming.js'

interface StreamingTextProps {
  text: string
  thinkingText: string
}

/**
 * Memoized frozen blocks — only re-renders when the rendered string changes.
 * Since blocks only accumulate (never shrink), this effectively freezes
 * completed output and prevents ink from redrawing it.
 */
const FrozenBlocks = memo(function FrozenBlocks({ rendered }: { rendered: string }) {
  if (!rendered) return null
  return <Text>{rendered}</Text>
})

export function StreamingText({ text, thinkingText }: StreamingTextProps) {
  if (text.length === 0 && thinkingText.length === 0) {
    return null
  }

  const boundaryRef = useRef(0)
  const frozenRef = useRef('')

  // Reset if text was replaced (component unmounts between turns)
  if (text.length < boundaryRef.current) {
    boundaryRef.current = 0
    frozenRef.current = ''
  }

  // Split new stable blocks from the tail
  const { stableTexts, newBoundary } = splitStableBlocks(text, boundaryRef.current)

  // Accumulate frozen rendered output
  if (stableTexts.length > 0) {
    for (const blockText of stableTexts) {
      const rendered = renderMarkdown(blockText)
      if (rendered.length > 0) {
        frozenRef.current += (frozenRef.current ? '\n' : '') + rendered
      }
    }
    boundaryRef.current = newBoundary
  }

  const frozenRendered = frozenRef.current
  const activeTail = text.substring(boundaryRef.current)
  const activeRendered = activeTail ? renderMarkdown(activeTail) : ''
  const hasContent = frozenRendered.length > 0 || activeRendered.length > 0

  return (
    <Box flexDirection="column" marginBottom={1}>
      {thinkingText.length > 0 && (
        <Box marginBottom={0}>
          <Text dimColor italic>
            {thinkingText}
          </Text>
        </Box>
      )}
      {hasContent && (
        <Box marginTop={1}>
          <Text color="magenta" bold>{'⏺ '}</Text>
          <Box flexDirection="column" flexShrink={1}>
            <FrozenBlocks rendered={frozenRendered.replace(/^\n+/, '')} />
            {activeRendered.length > 0 && (
              <Text>{activeRendered.replace(/^\n+/, '')}</Text>
            )}
            <Text color="gray">▍</Text>
          </Box>
        </Box>
      )}
    </Box>
  )
}
