/**
 * PromptInput component — text input with prompt prefix.
 * Handles user input, multiline, and submit.
 */

import React, { useState } from 'react'
import { Text, Box, useInput } from 'ink'

interface PromptInputProps {
  model: string
  isLoading: boolean
  onSubmit: (text: string) => void
  onInterrupt: () => void
}

export function PromptInput({ model, isLoading, onSubmit, onInterrupt }: PromptInputProps) {
  const [input, setInput] = useState('')

  useInput((ch, key) => {
    if (isLoading) {
      // Ctrl+C during loading → abort
      if (key.ctrl && ch === 'c') {
        onInterrupt()
      }
      return
    }

    if (key.ctrl && ch === 'c') {
      if (input.length === 0) {
        onInterrupt()
      } else {
        setInput('')
      }
      return
    }

    if (key.return) {
      const trimmed = input.trim()
      if (trimmed.length > 0) {
        onSubmit(trimmed)
        setInput('')
      }
      return
    }

    if (key.backspace || key.delete) {
      setInput((prev) => prev.slice(0, -1))
      return
    }

    // Ignore control sequences
    if (key.ctrl || key.meta) return
    if (key.escape) return

    // Regular character input
    if (ch) {
      setInput((prev) => prev + ch)
    }
  })

  if (isLoading) {
    return null
  }

  return (
    <Box>
      <Text backgroundColor="#5a2d82" color="white" bold>
        {' bendclaw '}
      </Text>
      <Text dimColor> {model} </Text>
      <Text bold color="yellow">{'> '}</Text>
      <Text>{input}</Text>
      <Text color="gray">█</Text>
    </Box>
  )
}
