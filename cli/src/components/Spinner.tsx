/**
 * Spinner component — animated loading indicator.
 * Inspired by Claude Code's spinner with shimmer effect.
 */

import React, { useState, useEffect } from 'react'
import { Text, Box } from 'ink'
import type { UIToolCall } from '../state/AppState.js'

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
const SPINNER_INTERVAL = 80

interface SpinnerProps {
  text?: string
  activeTools?: Map<string, UIToolCall>
}

export function Spinner({ text, activeTools }: SpinnerProps) {
  const [frame, setFrame] = useState(0)

  useEffect(() => {
    const timer = setInterval(() => {
      setFrame((prev) => (prev + 1) % SPINNER_FRAMES.length)
    }, SPINNER_INTERVAL)
    return () => clearInterval(timer)
  }, [])

  const runningTools = activeTools
    ? [...activeTools.values()].filter((t) => t.status === 'running')
    : []

  return (
    <Box flexDirection="column">
      <Box>
        <Text color="cyan">{SPINNER_FRAMES[frame]} </Text>
        <Text dimColor>{text ?? 'Thinking...'}</Text>
      </Box>
      {runningTools.map((tool) => (
        <Box key={tool.id} marginLeft={2}>
          <Text color="yellow">⟡ </Text>
          <Text dimColor>
            {tool.name}
            {tool.previewCommand ? `: ${tool.previewCommand}` : ''}
          </Text>
        </Box>
      ))}
    </Box>
  )
}
