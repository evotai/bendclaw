/**
 * StreamingText component — renders only the active (growing) tail block.
 *
 * Completed markdown blocks are reported via onFreezeBlocks callback so they
 * can be rendered in the parent's <Static> zone. This component only handles
 * the last incomplete block that needs dynamic re-rendering.
 */

import React, { useRef, useEffect } from 'react'
import { Text, Box } from 'ink'
import { renderMarkdown } from '../utils/markdown.js'
import { splitStableBlocks } from '../utils/streaming.js'

interface StreamingTextProps {
  text: string
  thinkingText: string
  onFreezeBlocks?: (blocks: string[]) => void
}

export function StreamingText({ text, thinkingText, onFreezeBlocks }: StreamingTextProps) {
  if (text.length === 0 && thinkingText.length === 0) {
    return null
  }

  const boundaryRef = useRef(0)

  // Reset if text was replaced (component unmounts between turns)
  if (text.length < boundaryRef.current) {
    boundaryRef.current = 0
  }

  // Split new stable blocks from the tail
  const { stableTexts, newBoundary } = splitStableBlocks(text, boundaryRef.current)

  // Report frozen blocks to parent for <Static> rendering
  if (stableTexts.length > 0) {
    boundaryRef.current = newBoundary
    // Fire in useEffect to avoid setState-during-render warnings
  }

  const pendingBlocksRef = useRef<string[]>([])
  if (stableTexts.length > 0) {
    pendingBlocksRef.current = stableTexts
  }

  useEffect(() => {
    if (pendingBlocksRef.current.length > 0 && onFreezeBlocks) {
      onFreezeBlocks(pendingBlocksRef.current)
      pendingBlocksRef.current = []
    }
  })

  // The active tail — only this part re-renders per delta
  const activeTail = text.substring(boundaryRef.current)
  const activeRendered = activeTail ? renderMarkdown(activeTail) : ''

  const hasFrozen = boundaryRef.current > 0

  return (
    <Box flexDirection="column" marginBottom={1}>
      {thinkingText.length > 0 && (
        <Box marginBottom={0}>
          <Text dimColor italic>
            {thinkingText}
          </Text>
        </Box>
      )}
      {activeRendered.length > 0 && (
        <Box marginTop={hasFrozen ? 0 : 1}>
          {!hasFrozen && <Text color="magenta" bold>{'⏺ '}</Text>}
          <Box flexDirection="column" flexShrink={1}>
            <Text>{activeRendered.replace(/^\n+/, '')}</Text>
            <Text color="gray">▍</Text>
          </Box>
        </Box>
      )}
      {activeRendered.length === 0 && (
        <Box>
          <Text color="gray">▍</Text>
        </Box>
      )}
    </Box>
  )
}
