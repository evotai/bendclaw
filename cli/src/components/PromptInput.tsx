/**
 * PromptInput component — bordered input box with status footer.
 * Modeled after Claude Code's PromptInput: rounded border, mode indicator,
 * footer with shortcuts and status info.
 */

import React, { useState, useRef } from 'react'
import { Text, Box, useInput } from 'ink'

interface PromptInputProps {
  model: string
  isLoading: boolean
  verbose: boolean
  onSubmit: (text: string) => void
  onInterrupt: () => void
  onToggleVerbose: () => void
}

export function PromptInput({
  model,
  isLoading,
  verbose,
  onSubmit,
  onInterrupt,
  onToggleVerbose,
}: PromptInputProps) {
  const [input, setInput] = useState('')
  const [cursorPos, setCursorPos] = useState(0)
  const historyRef = useRef<string[]>([])
  const historyIndexRef = useRef(-1)
  const savedInputRef = useRef('')

  useInput((ch, key) => {
    if (isLoading) {
      if (key.ctrl && ch === 'c') {
        onInterrupt()
      }
      return
    }

    // Ctrl+C — clear input or exit
    if (key.ctrl && ch === 'c') {
      if (input.length === 0) {
        onInterrupt()
      } else {
        setInput('')
        setCursorPos(0)
        historyIndexRef.current = -1
      }
      return
    }

    // Ctrl+D — exit if empty, otherwise delete char at cursor
    if (key.ctrl && ch === 'd') {
      if (input.length === 0) {
        onInterrupt()
      } else if (cursorPos < input.length) {
        setInput((prev) => prev.slice(0, cursorPos) + prev.slice(cursorPos + 1))
      }
      return
    }

    // Ctrl+U — clear from cursor to start
    if (key.ctrl && ch === 'u') {
      setInput((prev) => prev.slice(cursorPos))
      setCursorPos(0)
      return
    }

    // Ctrl+K — clear from cursor to end
    if (key.ctrl && ch === 'k') {
      setInput((prev) => prev.slice(0, cursorPos))
      return
    }

    // Ctrl+A — move to start
    if (key.ctrl && ch === 'a') {
      setCursorPos(0)
      return
    }

    // Ctrl+E — move to end
    if (key.ctrl && ch === 'e') {
      setCursorPos(input.length)
      return
    }

    // Ctrl+W — delete word backward
    if (key.ctrl && ch === 'w') {
      const before = input.slice(0, cursorPos)
      const trimmed = before.replace(/\s+$/, '')
      const lastSpace = trimmed.lastIndexOf(' ')
      const newPos = lastSpace === -1 ? 0 : lastSpace + 1
      setInput(input.slice(0, newPos) + input.slice(cursorPos))
      setCursorPos(newPos)
      return
    }

    // Ctrl+L — toggle verbose
    if (key.ctrl && ch === 'l') {
      onToggleVerbose()
      return
    }

    // Submit: Enter sends, Alt+Enter / Shift+Enter inserts newline
    if (key.return) {
      if (key.meta || key.shift) {
        // Insert newline at cursor
        setInput((prev) => prev.slice(0, cursorPos) + '\n' + prev.slice(cursorPos))
        setCursorPos((prev) => prev + 1)
        return
      }
      const trimmed = input.trim()
      if (trimmed.length > 0) {
        const history = historyRef.current
        if (history.length === 0 || history[history.length - 1] !== trimmed) {
          history.push(trimmed)
        }
        historyIndexRef.current = -1
        onSubmit(trimmed)
        setInput('')
        setCursorPos(0)
      }
      return
    }

    // Backspace
    if (key.backspace || key.delete) {
      if (cursorPos > 0) {
        setInput((prev) => prev.slice(0, cursorPos - 1) + prev.slice(cursorPos))
        setCursorPos((prev) => prev - 1)
      }
      return
    }

    // Arrow up — history previous
    if (key.upArrow) {
      const history = historyRef.current
      if (history.length === 0) return
      if (historyIndexRef.current === -1) {
        savedInputRef.current = input
        historyIndexRef.current = history.length - 1
      } else if (historyIndexRef.current > 0) {
        historyIndexRef.current--
      }
      const entry = history[historyIndexRef.current] ?? ''
      setInput(entry)
      setCursorPos(entry.length)
      return
    }

    // Arrow down — history next
    if (key.downArrow) {
      const history = historyRef.current
      if (historyIndexRef.current === -1) return
      if (historyIndexRef.current < history.length - 1) {
        historyIndexRef.current++
        const entry = history[historyIndexRef.current] ?? ''
        setInput(entry)
        setCursorPos(entry.length)
      } else {
        historyIndexRef.current = -1
        setInput(savedInputRef.current)
        setCursorPos(savedInputRef.current.length)
      }
      return
    }

    // Arrow left/right
    if (key.leftArrow) {
      setCursorPos((prev) => Math.max(0, prev - 1))
      return
    }
    if (key.rightArrow) {
      setCursorPos((prev) => Math.min(input.length, prev + 1))
      return
    }

    // Home / End
    if (key.home) {
      setCursorPos(0)
      return
    }
    if (key.end) {
      setCursorPos(input.length)
      return
    }

    // Tab — ignore
    if (key.tab) return

    // Ignore other control sequences
    if (key.ctrl || key.escape) return

    // Regular character input
    if (ch) {
      setInput((prev) => prev.slice(0, cursorPos) + ch + prev.slice(cursorPos))
      setCursorPos((prev) => prev + ch.length)
    }
  })

  if (isLoading) {
    return null
  }

  // Render input lines
  const lines = input.split('\n')
  const isMultiline = lines.length > 1

  // Calculate cursor line and column
  let charCount = 0
  let cursorLine = 0
  let cursorCol = 0
  for (let i = 0; i < lines.length; i++) {
    const lineLen = lines[i]!.length
    if (charCount + lineLen >= cursorPos && cursorPos <= charCount + lineLen) {
      cursorLine = i
      cursorCol = cursorPos - charCount
      break
    }
    charCount += lineLen + 1 // +1 for \n
  }

  return (
    <Box flexDirection="column" marginTop={1}>
      {/* Input box with border */}
      <Box
        borderStyle="round"
        borderColor="#7c3aed"
        borderLeft={false}
        borderRight={false}
        borderBottom
        flexDirection="column"
        width="100%"
      >
        {lines.map((line, lineIdx) => (
          <Box key={lineIdx} flexDirection="row">
            {lineIdx === 0 && (
              <Text color="#7c3aed" bold>{'❯ '}</Text>
            )}
            {lineIdx > 0 && (
              <Text dimColor>{'  '}</Text>
            )}
            {lineIdx === cursorLine ? (
              <InputLineWithCursor line={line} cursorCol={cursorCol} />
            ) : (
              <Text>{line}</Text>
            )}
          </Box>
        ))}
        {input.length === 0 && (
          <Box>
            <Text color="#7c3aed" bold>{'❯ '}</Text>
            <Text dimColor>Type a message...</Text>
          </Box>
        )}
      </Box>

      {/* Footer status line */}
      <PromptFooter model={model} verbose={verbose} isMultiline={isMultiline} />
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Input line with cursor rendering
// ---------------------------------------------------------------------------

function InputLineWithCursor({ line, cursorCol }: { line: string; cursorCol: number }) {
  const before = line.slice(0, cursorCol)
  const cursorChar = line[cursorCol] ?? ' '
  const after = line.slice(cursorCol + 1)

  return (
    <Text>
      {before}
      <Text inverse>{cursorChar}</Text>
      {after}
    </Text>
  )
}

// ---------------------------------------------------------------------------
// Footer — shortcuts and model info
// ---------------------------------------------------------------------------

function PromptFooter({
  model,
  verbose,
  isMultiline,
}: {
  model: string
  verbose: boolean
  isMultiline: boolean
}) {
  return (
    <Box flexDirection="row" justifyContent="space-between" width="100%">
      <Box>
        <Text dimColor>
          {isMultiline ? 'Alt+Enter newline · ' : ''}
          Ctrl+L {verbose ? 'brief' : 'verbose'} · /help
        </Text>
      </Box>
      <Box>
        <Text dimColor>{model}</Text>
      </Box>
    </Box>
  )
}
