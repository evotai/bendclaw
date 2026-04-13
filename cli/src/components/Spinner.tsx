/**
 * Spinner component — animated loading indicator.
 */

import React, { useState, useEffect } from 'react'
import { Text, Box } from 'ink'

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
const SPINNER_INTERVAL = 80

interface SpinnerProps {
  text?: string
}

export function Spinner({ text }: SpinnerProps) {
  const [frame, setFrame] = useState(0)

  useEffect(() => {
    const timer = setInterval(() => {
      setFrame((prev) => (prev + 1) % SPINNER_FRAMES.length)
    }, SPINNER_INTERVAL)
    return () => clearInterval(timer)
  }, [])

  return (
    <Box>
      <Text color="cyan">{SPINNER_FRAMES[frame]} </Text>
      <Text dimColor>{text ?? 'Thinking...'}</Text>
    </Box>
  )
}
