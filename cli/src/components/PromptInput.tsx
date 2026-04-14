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
import { Text, Box, useInput, useStdout, useStdin } from 'ink'
import { complete, getGhostHint, getCommandHints, type CommandHint } from '../commands/completion.js'
import type { HistoryManager } from '../utils/history.js'
import { InterruptHandler } from '../utils/interrupt.js'
import { needsContinuation } from '../utils/continuation.js'
import { getFreezeInputMode } from '../utils/freezeMode.js'

interface PromptInputProps {
  model: string
  isLoading: boolean
  isActive: boolean
  isFrozen: boolean
  fullscreenEnabled?: boolean
  verbose: boolean
  planning: boolean
  queuedMessages: string[]
  history: HistoryManager
  onSubmit: (text: string) => void
  onInterrupt: () => void
  onToggleFreeze: () => void
  onToggleVerbose: () => void
  onPageUp?: () => void
  onPageDown?: () => void
  onHome?: () => void
  onEnd?: () => void
}

export function PromptInput({
  model,
  isLoading,
  isActive,
  isFrozen,
  fullscreenEnabled = false,
  verbose,
  planning,
  queuedMessages,
  history,
  onSubmit,
  onInterrupt,
  onToggleFreeze,
  onToggleVerbose,
  onPageUp,
  onPageDown,
  onHome,
  onEnd,
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
  const { stdout } = useStdout()
  const { stdin, setRawMode, isRawModeSupported } = useStdin()
  const columns = stdout?.columns ?? 120
  const freezeMode = getFreezeInputMode(isActive, isFrozen)

  // Load persistent history on mount
  useEffect(() => {
    historyRef.current = history.load()
  }, [history])

  const currentText = () => lines.join('\n')
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
  }

  useEffect(() => {
    if (!isRawModeSupported) return
    setRawMode(freezeMode.shouldUseRawMode)
  }, [freezeMode.shouldUseRawMode, isRawModeSupported, setRawMode])

  useEffect(() => {
    if (!freezeMode.resumeOnLineInput) return

    const handleData = (data: string | Buffer) => {
      const text = typeof data === 'string' ? data : data.toString('utf8')
      if (text === '\r' || text === '\n' || text === '\r\n') {
        onToggleFreeze()
      }
    }

    stdin.ref()
    stdin.resume()
    stdin.on('data', handleData)
    return () => {
      stdin.off('data', handleData)
    }
  }, [freezeMode.resumeOnLineInput, stdin, onToggleFreeze])

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

    // Ctrl+S — freeze/unfreeze repaint so terminal selection is stable
    if (key.ctrl && ch === 's') {
      onToggleFreeze()
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

    if (fullscreenEnabled && key.pageUp) {
      onPageUp?.()
      return
    }

    if (fullscreenEnabled && key.pageDown) {
      onPageDown?.()
      return
    }

    if (fullscreenEnabled && (ch === '\x1b[H' || ch === '\x1b[1~')) {
      onHome?.()
      return
    }

    if (fullscreenEnabled && (ch === '\x1b[F' || ch === '\x1b[4~')) {
      onEnd?.()
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

    // Regular character input
    if (ch) {
      setCompletionCandidates([])
      setLines((prev) => {
        const newLines = [...prev]
        const line = newLines[cursorLine]!
        newLines[cursorLine] = line.slice(0, cursorCol) + ch + line.slice(cursorCol)
        return newLines
      })
      setCursorCol((prev) => prev + ch.length)
    }
  }, { isActive: freezeMode.shouldCaptureInput })

  const borderLine = '─'.repeat(columns)

  return (
    <Box flexDirection="column">
      {/* Top border */}
      <Text dimColor>{borderLine}</Text>

      {/* Input area */}
      {isFrozen ? (
        <Box>
          <Text color="yellow" bold>{'❯ '}</Text>
          <Text dimColor>Selection mode active. Press Enter to resume live updates.</Text>
        </Box>
      ) : (
        lines.map((line, lineIdx) => (
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
        ))
      )}

      {/* Completion candidates (file paths) */}
      {!isFrozen && completionCandidates.length > 1 && !lines[cursorLine]?.startsWith('/') && (
        <Box>
          <Text dimColor>  </Text>
          <Text dimColor>{completionCandidates.join('  ')}</Text>
        </Box>
      )}

      {/* Bottom border */}
      <Text dimColor>{borderLine}</Text>

      {/* Command hints dropdown */}
      {(() => {
        const line = lines[cursorLine] ?? ''
        const hints = getCommandHints(line, cursorCol)
        if (isFrozen || hints.length === 0 || !line.startsWith('/')) return null
        // Find max command name length for alignment
        const maxLen = Math.max(...hints.map(h => h.name.length))
        return (
          <Box flexDirection="column">
            {hints.map((h) => (
              <Box key={h.name}>
                <Text dimColor>  </Text>
                <Text color="cyan">{h.name.padEnd(maxLen + 2)}</Text>
                <Text dimColor>{h.description.length > 80 ? h.description.slice(0, 79) + '…' : h.description}</Text>
              </Box>
            ))}
          </Box>
        )
      })()}

      {/* Exit hint */}
      {!isFrozen && exitHint && (
        <Box>
          <Text dimColor italic>  Press Ctrl+C again to exit</Text>
        </Box>
      )}

      {/* Queued messages */}
      {!isFrozen && queuedMessages.length > 0 && (
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
      <Footer
        model={model}
        frozen={isFrozen}
        planning={planning}
        verbose={verbose}
        columns={columns}
      />
    </Box>
  )
}

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
// Footer — shortcuts hint + model name
// ---------------------------------------------------------------------------

function Footer({
  model,
  frozen,
  planning,
  verbose,
  columns,
}: {
  model: string
  frozen: boolean
  planning: boolean
  verbose: boolean
  columns: number
}) {
  // Compute actual rendered width for gap calculation
  let leftLen = 5 // "/help"
  if (planning) leftLen += 7 // " [plan]"
  if (verbose) leftLen += 10 // " [verbose]"
  if (frozen) leftLen += 9 // " [forzen]"
  const gap = Math.max(1, columns - leftLen - model.length)

  return (
    <Box>
      <Text dimColor>/help</Text>
      {planning && <Text color="yellow" bold>{' [plan]'}</Text>}
      {verbose && <Text color="cyan">{' [verbose]'}</Text>}
      {frozen && <Text color="yellow" bold>{' [forzen]'}</Text>}
      <Text>{' '.repeat(gap)}</Text>
      <Text dimColor>{model}</Text>
    </Box>
  )
}
