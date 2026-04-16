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
import type { ServerState } from '../repl/server.js'
import { formatUptime } from '../repl/server.js'
import { InterruptHandler } from '../input/interrupt.js'
import { needsContinuation } from '../input/continuation.js'
import {
  formatPastedTextRef,
  parsePasteRefs,
  expandPasteRefs,
  snapCursor,
  deleteRefBackspace,
  skipRefOnMove,
  shouldCollapse,
  cleanPastedText,
} from '../input/paste_refs.js'

interface PromptInputProps {
  model: string
  isLoading: boolean
  isActive: boolean
  verbose: boolean
  planning: boolean
  logMode: boolean
  queuedMessages: string[]
  history: HistoryManager
  updateHint?: string
  serverState: ServerState | null
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
  updateHint,
  serverState,
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

  // Snap cursor out of paste refs (e.g. after up/down arrow lands inside one)
  useEffect(() => {
    const line = lines[cursorLine]
    if (!line) return
    const refs = parsePasteRefs(line)
    const snapped = snapCursor(cursorCol, refs)
    if (snapped !== cursorCol) {
      setCursorCol(snapped)
    }
  }, [cursorCol, cursorLine, lines])

  const currentText = () => {
    const text = lines.join('\n')
    return expandPasteRefs(text, pastedChunksRef.current)
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
    // Discard terminal protocol responses that Ink's keypress parser doesn't
    // recognize (e.g. Kitty keyboard [?0u, DECRPM [?2026;2$y).
    // These leak through as character input and cause infinite re-render loops.
    if (ch && /^\[[\?=]/.test(ch)) return

    // During loading, Ctrl+C or Escape interrupts the stream
    if (isLoading) {
      if ((key.ctrl && ch === 'c') || key.escape) {
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
        // Add display text (with placeholders) to history so up/down
        // arrow restores the collapsed single-line view.
        const displayText = lines.join('\n').trim()
        const hist = historyRef.current
        if (hist.length === 0 || hist[hist.length - 1] !== displayText) {
          hist.push(displayText)
        }
        history.append(displayText)
        historyIndexRef.current = -1
        onSubmit(text)
        clearInput()
      }
      return
    }

    // Backspace
    if (key.backspace || key.delete) {
      if (cursorCol > 0) {
        // Check if we should delete an entire paste ref
        const currentLine = lines[cursorLine]!
        const refs = parsePasteRefs(currentLine)
        const refDel = deleteRefBackspace(currentLine, cursorCol, refs)
        if (refDel) {
          const deletedRef = refs.find(r => r.end === cursorCol)
          if (deletedRef) {
            pastedChunksRef.current.delete(deletedRef.id)
          }
          setLines((prev) => {
            const newLines = [...prev]
            newLines[cursorLine] = refDel.newLine
            return newLines
          })
          setCursorCol(refDel.newCursorCol)
        } else {
          setLines((prev) => {
            const newLines = [...prev]
            const line = newLines[cursorLine]!
            newLines[cursorLine] = line.slice(0, cursorCol - 1) + line.slice(cursorCol)
            return newLines
          })
          setCursorCol((prev) => prev - 1)
        }
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
          savedInputRef.current = lines.join('\n')
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

    // Arrow left/right — skip over paste refs
    if (key.leftArrow) {
      if (cursorCol > 0) {
        const refs = parsePasteRefs(lines[cursorLine]!)
        const skip = skipRefOnMove(cursorCol, 'left', refs)
        setCursorCol(skip ?? cursorCol - 1)
      } else if (cursorLine > 0) {
        setCursorLine((prev) => prev - 1)
        setCursorCol(lines[cursorLine - 1]!.length)
      }
      return
    }
    if (key.rightArrow) {
      const lineLen = lines[cursorLine]!.length
      if (cursorCol < lineLen) {
        const refs = parsePasteRefs(lines[cursorLine]!)
        const skip = skipRefOnMove(cursorCol, 'right', refs)
        setCursorCol(skip ?? cursorCol + 1)
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
      const cleaned = cleanPastedText(ch)
      const pastedLines = cleaned.split('\n')

      // Collapse large pastes into a placeholder to avoid terminal
      // rendering jitter and keep history navigation working (up/down arrows
      // only navigate history when lines.length === 1).
      if (shouldCollapse(cleaned)) {
        const id = nextPasteIdRef.current++
        const numLines = (cleaned.match(/\n/g) || []).length
        pastedChunksRef.current.set(id, cleaned)
        const ref = formatPastedTextRef(id, numLines)
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
          newLines[cursorLine] = line.slice(0, cursorCol) + cleaned + line.slice(cursorCol)
          return newLines
        })
        setCursorCol((prev) => prev + cleaned.length)
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
            <DimRefsLine text={line || ' '} />
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
      <Footer model={model} planning={planning} logMode={logMode} updateHint={updateHint} serverState={serverState} columns={columns} />
    </Box>
  )
})

// ---------------------------------------------------------------------------
// CursorLine — renders a line with an inverse cursor at the right position.
// Paste refs like [Pasted text #1 +5 lines] are rendered with dim color.
// ---------------------------------------------------------------------------

function CursorLine({ text, cursorCol, ghostHint }: { text: string; cursorCol: number; ghostHint?: string }) {
  const refs = parsePasteRefs(text)
  const cursorChar = text[cursorCol] ?? ' '

  // Build segments: split text into normal parts and paste ref parts
  const segments: { text: string; dim: boolean }[] = []
  let pos = 0
  for (const ref of refs) {
    if (ref.start > pos) {
      segments.push({ text: text.slice(pos, ref.start), dim: false })
    }
    segments.push({ text: ref.match, dim: true })
    pos = ref.end
  }
  if (pos < text.length) {
    segments.push({ text: text.slice(pos), dim: false })
  }

  // Render segments with cursor overlay
  const parts: React.ReactNode[] = []
  let charIdx = 0
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i]!
    const segStart = charIdx
    const segEnd = charIdx + seg.text.length

    if (cursorCol >= segStart && cursorCol < segEnd) {
      // Cursor is inside this segment
      const localCol = cursorCol - segStart
      const before = seg.text.slice(0, localCol)
      const after = seg.text.slice(localCol + 1)
      if (seg.dim) {
        parts.push(
          <Text key={i} dimColor>{before}<Text inverse>{cursorChar}</Text>{after}</Text>
        )
      } else {
        parts.push(
          <Text key={i}>{before}<Text inverse>{cursorChar}</Text>{after}</Text>
        )
      }
    } else {
      parts.push(
        <Text key={i} dimColor={seg.dim}>{seg.text}</Text>
      )
    }
    charIdx = segEnd
  }

  // Cursor is at end of line (past all segments)
  if (cursorCol >= charIdx) {
    parts.push(<Text key="cursor" inverse>{cursorChar}</Text>)
  }

  return (
    <Text>
      {parts}
      {ghostHint ? <Text dimColor>{ghostHint}</Text> : null}
    </Text>
  )
}

// ---------------------------------------------------------------------------
// DimRefsLine — renders a non-cursor line with paste refs dimmed
// ---------------------------------------------------------------------------

function DimRefsLine({ text }: { text: string }) {
  const refs = parsePasteRefs(text)
  if (refs.length === 0) return <Text>{text}</Text>

  const parts: React.ReactNode[] = []
  let pos = 0
  for (let i = 0; i < refs.length; i++) {
    const ref = refs[i]!
    if (ref.start > pos) {
      parts.push(<Text key={`t${i}`}>{text.slice(pos, ref.start)}</Text>)
    }
    parts.push(<Text key={`r${i}`} dimColor>{ref.match}</Text>)
    pos = ref.end
  }
  if (pos < text.length) {
    parts.push(<Text key="tail">{text.slice(pos)}</Text>)
  }
  return <Text>{parts}</Text>
}

// ---------------------------------------------------------------------------
// Footer — model name + mode indicators
// ---------------------------------------------------------------------------

function Footer({ model, planning, logMode, updateHint, serverState, columns }: {
  model: string
  planning: boolean
  logMode: boolean
  updateHint?: string
  serverState: ServerState | null
  columns: number
}) {
  const [, setTick] = useState(0)

  useEffect(() => {
    if (!serverState) return
    const timer = setInterval(() => setTick((t) => t + 1), 1000)
    return () => clearInterval(timer)
  }, [serverState])

  const hints: string[] = []
  if (logMode) hints.push('[log] /done to exit')
  if (planning) hints.push('[plan]')
  const left = hints.join('  ')
  const serverLabel = serverState ? `  [server :${serverState.port} · ${formatUptime(serverState.startedAt)}]` : ''
  const right = model + serverLabel
  const gap = Math.max(1, columns - left.length - right.length)

  return (
    <Box flexDirection="column">
      {updateHint && (
        <Box>
          <Text color="yellow">{'  '}{updateHint}</Text>
        </Box>
      )}
      <Box>
        {logMode && <Text color="magenta" bold>{'[log]'}</Text>}
        {logMode && <Text dimColor>{' /done to exit'}</Text>}
        {planning && <Text color="yellow" bold>{logMode ? '  [plan]' : '[plan]'}</Text>}
        <Text>{' '.repeat(gap)}</Text>
        <Text dimColor>{model}</Text>
        {serverState && (
          <Text dimColor>{`  [server :${serverState.port} · ${formatUptime(serverState.startedAt)}]`}</Text>
        )}
      </Box>
    </Box>
  )
}
