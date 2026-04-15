/**
 * PromptInput component — Claude Code-style bordered input box.
 * 
 * Layout:
 *   ────────────────────────────────────
 *   ❯ user input text here
 *   ────────────────────────────────────
 *   ? for shortcuts                model
 */

import React, { useState, useRef, useEffect } from 'react'
import { Text, Box, useInput, useStdout } from 'ink'
import { complete, getGhostHint } from '../commands/completion.js'
import type { HistoryManager } from '../session/history.js'
import { InterruptHandler } from '../input/interrupt.js'
import { needsContinuation } from '../input/continuation.js'

interface PromptInputProps {
  model: string
  isLoading: boolean
  isActive: boolean
  verbose: boolean
  planning: boolean
  logMode: boolean
  queuedMessages: string[]
  history: HistoryManager
  onSubmit: (text: string) => void
  onInterrupt: () => void
  onToggleVerbose: () => void
}

export const PromptInput = React.memo(function PromptInput({
  model,
  isLoading,
  isActive,
  verbose,
  planning,
  logMode,
  queuedMessages,
  history,
  onSubmit,
  onInterrupt,
  onToggleVerbose,
}: PromptInputProps) {
  const [lines, setLines] = useState<string[]>([''])
  const [cursorLine, setCursorLine] = useState(0)
  const [cursorCol, setCursorCol] = useState(0)
  const [completionCandidates, setCompletionCandidates] = useState<string[]>([])
  const [exitHint, setExitHint] = useState(false)
  const historyRef = useRef<string[]>([])
  const historyIndexRef = useRef(-1)
  const savedInputRef = useRef('')
  const interruptRef = useRef(new InterruptHandler())
  // When a large paste is collapsed, store the full text here.
  const pastedChunksRef = useRef<Map<number, string>>(new Map())
  const nextPasteIdRef = useRef(1)
  const { stdout } = useStdout()
  const columns = stdout?.columns ?? 120

  // Load persistent history on mount
  useEffect(() => {
    historyRef.current = history.load()
  }, [history])

  const currentText = () => {
    let text = lines.join('\n')
    // Expand paste placeholders: [Pasted text #N] or [Pasted text #N +M lines]
    const refPattern = /\[Pasted text #(\d+)(?:\s\+\d+ lines)?\]/g
    text = text.replace(refPattern, (match, idStr) => {
      const id = parseInt(idStr, 10)
      return pastedChunksRef.current.get(id) ?? match
    })
    return text
  }
  const setInputText = (text: string) => {
    const newLines = text.split('\n')
    setLines(newLines)
    const lastLine = newLines.length - 1
    setCursorLine(lastLine)
    setCursorCol(newLines[lastLine]!.length)
  }

  const clearInput = () => {
    setLines([''])
    setCursorLine(0)
    setCursorCol(0)
    historyIndexRef.current = -1
    pastedChunksRef.current.clear()
  }

  useInput((ch, key) => {
    // During loading, only allow Ctrl+C to interrupt and Enter to queue
    if (isLoading) {
      if (key.ctrl && ch === 'c') {
        onInterrupt()
        return
      }
      // Allow typing into the input while loading — fall through
    }

    // Ctrl+C — clear input, show exit hint, or exit
    if (key.ctrl && ch === 'c') {
      const action = interruptRef.current.onInterrupt(currentText().length === 0)
      if (action === 'exit') {
        setExitHint(false)
        onInterrupt()
      } else if (action === 'show_hint') {
        setExitHint(true)
        setTimeout(() => setExitHint(false), 1500)
      } else {
        clearInput()
        setExitHint(false)
      }
      return
    }

    // Any normal input cancels pending exit hint
    interruptRef.current.onInput()
    if (exitHint) setExitHint(false)

    // Ctrl+L — clear all input (form feed \x0c)
    if ((key.ctrl && ch === 'l') || ch === '\f') {
      clearInput()
      return
    }

    // Ctrl+U — clear line before cursor
    if (key.ctrl && ch === 'u') {
      setLines((prev) => {
        const newLines = [...prev]
        newLines[cursorLine] = newLines[cursorLine]!.slice(cursorCol)
        return newLines
      })
      setCursorCol(0)
      return
    }

    // Ctrl+K — clear line after cursor
    if (key.ctrl && ch === 'k') {
      setLines((prev) => {
        const newLines = [...prev]
        newLines[cursorLine] = newLines[cursorLine]!.slice(0, cursorCol)
        return newLines
      })
      return
    }

    // Ctrl+O — toggle verbose output
    if ((key.ctrl && ch === 'o') || ch === '\x0f') {
      onToggleVerbose()
      return
    }

    // Ctrl+A — move to start of line
    if (key.ctrl && ch === 'a') {
      setCursorCol(0)
      return
    }

    // Ctrl+E — move to end of line
    if (key.ctrl && ch === 'e') {
      setCursorCol(lines[cursorLine]!.length)
      return
    }

    // Ctrl+D — delete char at cursor (or exit if empty)
    if (key.ctrl && ch === 'd') {
      const line = lines[cursorLine]!
      if (currentText().length === 0) {
        onInterrupt()
        return
      }
      if (cursorCol < line.length) {
        setLines((prev) => {
          const newLines = [...prev]
          newLines[cursorLine] = line.slice(0, cursorCol) + line.slice(cursorCol + 1)
          return newLines
        })
      } else if (cursorLine < lines.length - 1) {
        // Join with next line
        setLines((prev) => {
          const newLines = [...prev]
          newLines[cursorLine] = newLines[cursorLine]! + newLines[cursorLine + 1]!
          newLines.splice(cursorLine + 1, 1)
          return newLines
        })
      }
      return
    }

    // Ctrl+W — delete word before cursor (standard bash unix-word-rubout)
    if (key.ctrl && ch === 'w') {
      const line = lines[cursorLine]!
      let i = cursorCol
      // skip trailing whitespace backward
      while (i > 0 && line[i - 1] === ' ') i--
      // skip word backward
      while (i > 0 && line[i - 1] !== ' ') i--
      const newCol = i
      setLines((prev) => {
        const newLines = [...prev]
        newLines[cursorLine] = line.slice(0, newCol) + line.slice(cursorCol)
        return newLines
      })
      setCursorCol(newCol)
      return
    }

    // Enter — submit (single line) or newline (if Alt/Option+Enter)
    if (key.return) {
      if (key.meta) {
        // Alt+Enter → insert newline
        setLines((prev) => {
          const line = prev[cursorLine]!
          const newLines = [...prev]
          newLines.splice(cursorLine, 1, line.slice(0, cursorCol), line.slice(cursorCol))
          return newLines
        })
        setCursorLine((prev) => prev + 1)
        setCursorCol(0)
        return
      }

      const text = currentText().trim()
      if (text.length > 0) {
        // Check for continuation (unclosed fences, trailing backslash)
        if (needsContinuation(text)) {
          // Auto-insert newline instead of submitting
          setLines((prev) => {
            const line = prev[cursorLine]!
            const newLines = [...prev]
            newLines.splice(cursorLine, 1, line.slice(0, cursorCol), line.slice(cursorCol))
            return newLines
          })
          setCursorLine((prev) => prev + 1)
          setCursorCol(0)
          return
        }
        // Add to in-memory + persistent history
        const hist = historyRef.current
        if (hist.length === 0 || hist[hist.length - 1] !== text) {
          hist.push(text)
        }
        history.append(text)
        historyIndexRef.current = -1
        onSubmit(text)
        clearInput()
      }
      return
    }

    // Backspace
    if (key.backspace || key.delete) {
      if (cursorCol > 0) {
        setLines((prev) => {
          const newLines = [...prev]
          const line = newLines[cursorLine]!
          newLines[cursorLine] = line.slice(0, cursorCol - 1) + line.slice(cursorCol)
          return newLines
        })
        setCursorCol((prev) => prev - 1)
      } else if (cursorLine > 0) {
        // Join with previous line
        const prevLineLen = lines[cursorLine - 1]!.length
        setLines((prev) => {
          const newLines = [...prev]
          newLines[cursorLine - 1] = newLines[cursorLine - 1]! + newLines[cursorLine]!
          newLines.splice(cursorLine, 1)
          return newLines
        })
        setCursorLine((prev) => prev - 1)
        setCursorCol(prevLineLen)
      }
      return
    }

    // Arrow up — history or move cursor up
    if (key.upArrow) {
      if (lines.length === 1) {
        // Single line → navigate history
        const history = historyRef.current
        if (history.length === 0) return
        if (historyIndexRef.current === -1) {
          savedInputRef.current = currentText()
          historyIndexRef.current = history.length - 1
        } else if (historyIndexRef.current > 0) {
          historyIndexRef.current--
        }
        setInputText(history[historyIndexRef.current] ?? '')
      } else if (cursorLine > 0) {
        setCursorLine((prev) => prev - 1)
        setCursorCol((prev) => Math.min(prev, lines[cursorLine - 1]!.length))
      }
      return
    }

    // Arrow down — history or move cursor down
    if (key.downArrow) {
      if (lines.length === 1) {
        const history = historyRef.current
        if (historyIndexRef.current === -1) return
        if (historyIndexRef.current < history.length - 1) {
          historyIndexRef.current++
          setInputText(history[historyIndexRef.current] ?? '')
        } else {
          historyIndexRef.current = -1
          setInputText(savedInputRef.current)
        }
      } else if (cursorLine < lines.length - 1) {
        setCursorLine((prev) => prev + 1)
        setCursorCol((prev) => Math.min(prev, lines[cursorLine + 1]!.length))
      }
      return
    }

    // Arrow left/right
    if (key.leftArrow) {
      if (cursorCol > 0) {
        setCursorCol((prev) => prev - 1)
      } else if (cursorLine > 0) {
        setCursorLine((prev) => prev - 1)
        setCursorCol(lines[cursorLine - 1]!.length)
      }
      return
    }
    if (key.rightArrow) {
      const lineLen = lines[cursorLine]!.length
      if (cursorCol < lineLen) {
        setCursorCol((prev) => prev + 1)
      } else if (cursorLine < lines.length - 1) {
        setCursorLine((prev) => prev + 1)
        setCursorCol(0)
      }
      return
    }

    // Tab — completion
    if (key.tab) {
      const line = lines[cursorLine]!
      const result = complete(line, cursorCol)
      if (result) {
        if (result.candidates.length === 1) {
          // Single match — apply and clear candidates
          setLines((prev) => {
            const newLines = [...prev]
            const l = newLines[cursorLine]!
            newLines[cursorLine] = l.slice(0, result.wordStart) + result.replacement + l.slice(cursorCol)
            return newLines
          })
          setCursorCol(result.wordStart + result.replacement.length)
          setCompletionCandidates([])
        } else {
          // Multiple matches — apply common prefix and show candidates
          setLines((prev) => {
            const newLines = [...prev]
            const l = newLines[cursorLine]!
            newLines[cursorLine] = l.slice(0, result.wordStart) + result.replacement + l.slice(cursorCol)
            return newLines
          })
          setCursorCol(result.wordStart + result.replacement.length)
          setCompletionCandidates(result.candidates)
        }
      }
      return
    }

    // Ignore other control sequences
    if (key.ctrl || key.escape) return

    // Regular character input (including multi-line paste)
    if (ch) {
      setCompletionCandidates([])
      const normalized = ch.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
      const pastedLines = normalized.split('\n')

      // Collapse large pastes into a placeholder to avoid terminal
      // rendering jitter. Full content is stored in pastedChunksRef
      // and expanded back on submit via currentText().
      const PASTE_CHAR_THRESHOLD = 1000
      const PASTE_LINE_THRESHOLD = 3
      const numLines = (normalized.match(/\n/g) || []).length

      if (pastedLines.length > 1 && (normalized.length > PASTE_CHAR_THRESHOLD || numLines > PASTE_LINE_THRESHOLD)) {
        const id = nextPasteIdRef.current++
        pastedChunksRef.current.set(id, normalized)
        const ref = numLines === 0
          ? `[Pasted text #${id}]`
          : `[Pasted text #${id} +${numLines} lines]`
        setLines((prev) => {
          const newLines = [...prev]
          const line = newLines[cursorLine]!
          const before = line.slice(0, cursorCol)
          const after = line.slice(cursorCol)
          newLines[cursorLine] = before + ref + after
          return newLines
        })
        setCursorCol((prev) => prev + ref.length)
      } else if (pastedLines.length > 1) {
        // Multi-line paste (small enough to display inline)
        setLines((prev) => {
          const newLines = [...prev]
          const line = newLines[cursorLine]!
          const before = line.slice(0, cursorCol)
          const after = line.slice(cursorCol)
          const spliced: string[] = [
            before + pastedLines[0]!,
            ...pastedLines.slice(1, -1),
            pastedLines[pastedLines.length - 1]! + after,
          ]
          newLines.splice(cursorLine, 1, ...spliced)
          return newLines
        })
        const lastPasted = pastedLines[pastedLines.length - 1]!
        setCursorLine((prev) => prev + pastedLines.length - 1)
        setCursorCol(lastPasted.length)
      } else {
        setLines((prev) => {
          const newLines = [...prev]
          const line = newLines[cursorLine]!
          newLines[cursorLine] = line.slice(0, cursorCol) + ch + line.slice(cursorCol)
          return newLines
        })
        setCursorCol((prev) => prev + ch.length)
      }
    }
  }, { isActive })

  const borderLine = '─'.repeat(columns)

  return (
    <Box flexDirection="column">
      {/* Top border */}
      <Text dimColor>{borderLine}</Text>

      {/* Input area */}
      {lines.map((line, lineIdx) => (
        <Box key={lineIdx}>
          <Text color="cyan" bold>{lineIdx === 0 ? '❯ ' : '  '}</Text>
          {lineIdx === cursorLine ? (
            line === '' && lines.length === 1 ? (
              // Empty input — show placeholder with cursor
              <Text>
                <Text inverse>{' '}</Text>
                <Text dimColor>Type a message...</Text>
              </Text>
            ) : (
              <CursorLine text={line} cursorCol={cursorCol} ghostHint={getGhostHint(line, cursorCol)} />
            )
          ) : (
            <Text>{line || ' '}</Text>
          )}
        </Box>
      ))}

      {/* Completion candidates (file paths) */}
      {completionCandidates.length > 1 && !lines[cursorLine]?.startsWith('/') && (
        <Box>
          <Text dimColor>  </Text>
          <Text dimColor>{completionCandidates.join('  ')}</Text>
        </Box>
      )}

      {/* Bottom border */}
      <Text dimColor>{borderLine}</Text>

      {/* Exit hint */}
      {exitHint && (
        <Box>
          <Text dimColor italic>  Press Ctrl+C again to exit</Text>
        </Box>
      )}

      {/* Queued messages */}
      {queuedMessages.length > 0 && (
        <Box flexDirection="column" marginBottom={0}>
          {queuedMessages.map((msg, i) => (
            <Box key={i}>
              <Text dimColor>  ❯ </Text>
              <Text dimColor>{msg}</Text>
            </Box>
          ))}
        </Box>
      )}

      {/* Footer */}
      <Footer model={model} planning={planning} logMode={logMode} columns={columns} />
    </Box>
  )
})

// ---------------------------------------------------------------------------
// CursorLine — renders a line with an inverse cursor at the right position
// ---------------------------------------------------------------------------

function CursorLine({ text, cursorCol, ghostHint }: { text: string; cursorCol: number; ghostHint?: string }) {
  const before = text.slice(0, cursorCol)
  const cursorChar = text[cursorCol] ?? ' '
  const after = text.slice(cursorCol + 1)

  return (
    <Text>
      {before}
      <Text inverse>{cursorChar}</Text>
      {after}
      {ghostHint ? <Text dimColor>{ghostHint}</Text> : null}
    </Text>
  )
}

// ---------------------------------------------------------------------------
// Footer — model name + mode indicators
// ---------------------------------------------------------------------------

function Footer({ model, planning, logMode, columns }: {
  model: string
  planning: boolean
  logMode: boolean
  columns: number
}) {
  const hints: string[] = []
  if (logMode) hints.push('[log] /done to exit')
  if (planning) hints.push('[plan]')
  const left = hints.join('  ')
  const gap = Math.max(1, columns - left.length - model.length)

  return (
    <Box>
      {logMode && <Text color="magenta" bold>{'[log]'}</Text>}
      {logMode && <Text dimColor>{' /done to exit'}</Text>}
      {planning && <Text color="yellow" bold>{logMode ? '  [plan]' : '[plan]'}</Text>}
      <Text>{' '.repeat(gap)}</Text>
      <Text dimColor>{model}</Text>
    </Box>
  )
}
