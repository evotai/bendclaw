import { TermRenderer } from './renderer.js'
import { parseInput, enableRawMode, type KeyEvent } from './input.js'
import { installBracketedPaste } from './bracketed-paste.js'
import { createSpinnerState, advanceSpinner, type SpinnerState } from './spinner.js'
import { createSelectorState, selectorUp, selectorDown, selectorSelect, selectorType, selectorBackspace, type SelectorState } from './selector.js'
import { createAskState, handleAskKeyEvent, type AskState, type AskQuestion } from './ask.js'
import { buildUserMessage, buildAssistantLines, type OutputLine } from '../render/output.js'
import { Agent, QueryStream, type SessionMeta, type ConfigInfo } from '../native/index.js'
import { createInitialState, type AppState } from './app/state.js'
import type { AskUserRequest } from './app/types.js'
import { HistoryManager } from '../session/history.js'
import { ScreenLog } from '../session/screen-log.js'
import { isSlashCommand, resolveCommand } from '../commands/index.js'
import { renderBanner } from './banner.js'
import { relativeTime } from '../render/format.js'
import {
  buildOutputBlocks,
  buildActiveResponseBlocks,
  buildPromptBlocks,
  buildOverlayBlocks,
  buildAskBlocks,
  blocksToLines,
  type OverlayState,
  type PromptVMInput,
} from './viewmodel/index.js'
import {
  createEditorState,
  getEditorText,
  isEditorEmpty,
  clearEditor,
  insertText,
  backspace,
  moveLeft,
  moveRight,
  moveHome,
  moveEnd,
  applyCompletion,
  refreshGhostHint,
  createHistoryState,
  pushHistory,
  historyPrev,
  historyNext,
  clearLineBefore,
  clearLineAfter,
  deleteForward,
  deleteWordBefore,
  insertNewline,
  moveUp,
  moveDown,
  editorNeedsContinuation,
  type EditorState,
  type HistoryState,
} from './input/editor.js'
import {
  createStreamMachineState,
  reduceRunEvent,
  flushStreaming,
  buildToolStartedLines,
  buildToolFinishedLines,
  type StreamMachineState,
} from './app/stream.js'
import { handleSlashCommand } from './app/commands.js'
import { askStateToResponse } from './app/ask-user.js'
import { syncProvider } from './app/provider.js'
import chalk from 'chalk'
import {
  shouldCollapse,
  cleanPastedText,
  formatPastedTextRef,
  formatImageRef,
  parsePasteRefs,
  expandPasteRefs,
  stripImageRefs,
  deleteRefBackspace,
  skipRefOnMove,
  snapCursor,
} from './input/paste_refs.js'
import { getImageFromClipboard, type ClipboardImage } from './input/clipboard_image.js'
import type { ContentBlock } from '../native/index.js'
import { tryStartServer, formatUptime, type ServerState } from './app/server.js'

const SPINNER_INTERVAL_MS = 100

export interface ReplOptions {
  agent: Agent
  verbose?: boolean
  resumeSessionId?: string
  serverPort?: number
  envFile?: string
}

export async function startRepl(opts: ReplOptions): Promise<void> {
  const { agent } = opts
  const renderer = new TermRenderer()
  renderer.init()

  let appState: AppState = {
    ...createInitialState(agent.model, agent.cwd),
    verbose: opts.verbose ?? true,
  }
  let spinnerState = createSpinnerState()
  let editor: EditorState = createEditorState()
  let historyState: HistoryState
  let isLoading = false
  let streamRef: QueryStream | null = null
  let spinnerTimer: ReturnType<typeof setInterval> | null = null
  let destroyed = false
  let sessionId: string | null = null
  let planning = false
  let logMode: import('../native/index.js').ForkedAgent | null = null
  let exitHint = false
  let exitHintTimer: ReturnType<typeof setTimeout> | null = null
  let overlay: OverlayState = { kind: 'none' }
  let streamMachine: StreamMachineState | null = null
  let expanded = false
  const compactLines: OutputLine[] = []
  const expandedLines: OutputLine[] = []
  let lastProgressLineCount = 0
  const screenLog = new ScreenLog()

  // Server state
  let serverState: ServerState | null = null
  try {
    serverState = await tryStartServer(opts.serverPort, opts.envFile)
  } catch { /* server start failed — continue without it */ }

  // Paste ref state
  const pastedChunks = new Map<number, string>()
  const pastedImages = new Map<number, { id: number; base64: string; mediaType: string }>()
  let nextPasteId = 1

  // Update hint
  let updateHint: string | null = null
  const updateMgr = new (await import('../update/index.js')).UpdateManager(
    (await import('../native/index.js')).version()
  )
  updateMgr.on('update-available', (info: { version: string }) => {
    updateHint = `v${info.version} available · /update`
    renderStatus()
  })
  updateMgr.start()

  const historyMgr = new HistoryManager()
  const entries = historyMgr.load()
  historyState = createHistoryState(entries)

  let configInfo: ConfigInfo | undefined
  try { configInfo = agent.configInfo() } catch {}

  let preloadedSessions: SessionMeta[] = []
  try { preloadedSessions = await agent.listSessions(20) } catch {}

  const bannerText = renderBanner(agent.model, agent.cwd, configInfo, preloadedSessions, renderer.termCols, serverState)
  renderer.appendScroll(bannerText)
  setTerminalTitle()

  if (opts.resumeSessionId) {
    const match = preloadedSessions.find(
      (s) => s.session_id === opts.resumeSessionId || s.session_id.startsWith(opts.resumeSessionId!)
    )
    if (match) {
      await resumeSession(match)
    } else {
      commitLines([{ id: 'sys-resume-err', kind: 'system', text: chalk.red(`Session not found: ${opts.resumeSessionId}`) }])
    }
  } else {
    const match = preloadedSessions.find((s) => s.cwd === agent.cwd)
    if (match) {
      const tag = match.source ? `[${match.source}] ` : ''
      const title = match.title || '(untitled)'
      const short = title.length > 40 ? title.slice(0, 39) + '…' : title
      commitLines([{
        id: `prev-session-${match.session_id}`,
        kind: 'system',
        text: `  previous session: ${tag}${short} · /resume ${match.session_id.slice(0, 8)}`,
      }])
    }
  }

  renderStatus()

  function getPromptVM(): PromptVMInput {
    return {
      lines: editor.lines,
      cursorLine: editor.cursorLine,
      cursorCol: editor.cursorCol,
      active: overlay.kind === 'none',
      model: appState.model,
      verbose: appState.verbose,
      planning,
      logMode: logMode !== null,
      queuedMessages: [],
      updateHint,
      serverUptime: serverState ? formatUptime(serverState.startedAt) : null,
      serverPort: serverState?.port ?? null,
      exitHint,
      completionCandidates: editor.completionCandidates,
      ghostHint: editor.ghostHint,
      columns: renderer.termCols,
      isLoading,
      placeholder: isEditorEmpty(editor) && !isLoading,
    }
  }

  function renderStatus() {
    if (destroyed) return
    const pendingText = streamMachine?.pendingText ?? ''
    const toolProgress = streamMachine?.toolProgress ?? ''

    // When ask-user is active, suppress the spinner — the agent is waiting
    // for user input, not "thinking" or "executing".
    const isAskUserActive = overlay.kind === 'ask-user'
    const activeBlocks = buildActiveResponseBlocks({
      isLoading: isAskUserActive ? false : isLoading,
      pendingText: isAskUserActive ? '' : pendingText,
      toolProgress: isAskUserActive ? '' : toolProgress,
      spinner: spinnerState,
      termRows: renderer.termRows,
      expanded,
      assistantCommitted: streamMachine?.assistantCommitted,
    })
    const overlayBlocks = buildOverlayBlocks(overlay, renderer.termCols)
    const promptBlocks = buildPromptBlocks(getPromptVM())
    const statusLines = blocksToLines([...activeBlocks, ...overlayBlocks, ...promptBlocks])
    renderer.setStatus(statusLines)
  }

  function commitLines(outputLines: OutputLine[]) {
    if (outputLines.length === 0) return
    const prevKind = compactLines.length > 0 ? compactLines[compactLines.length - 1]!.kind : undefined
    compactLines.push(...outputLines)
    expandedLines.push(...outputLines)
    const visible = expanded ? expandedLines.slice(-outputLines.length) : outputLines
    const blocks = buildOutputBlocks(visible, prevKind)
    renderer.beginBatch()
    renderer.appendScroll(blocksToLines(blocks).join('\n'))
    renderStatus()
    renderer.flushBatch()
    screenLog.log(outputLines)
  }

  /** Commit tool_finished with both compact and expanded versions. */
  function commitToolFinished(event: import('../native/index.js').RunEvent): void {
    const compact = buildToolFinishedLines(event)
    const exp = buildToolFinishedLines(event, true)
    const prevKind = compactLines.length > 0 ? compactLines[compactLines.length - 1]!.kind : undefined
    compactLines.push(...compact)
    expandedLines.push(...exp)
    const visible = expanded ? exp : compact
    const blocks = buildOutputBlocks(visible, prevKind)
    renderer.beginBatch()
    renderer.appendScroll(blocksToLines(blocks).join('\n'))
    renderStatus()
    renderer.flushBatch()
    screenLog.log(exp)
  }

  /** Toggle expanded view and redraw. */
  function toggleExpanded(): void {
    expanded = !expanded
    const lines = expanded ? expandedLines : compactLines
    renderer.beginBatch()
    renderer.clearScreen()
    if (lines.length > 0) {
      const blocks = buildOutputBlocks(lines)
      renderer.appendScroll(blocksToLines(blocks).join('\n'))
    }
    renderStatus()
    renderer.flushBatch()
  }

  function setTerminalTitle(suffix?: string) {
    const base = 'Evot'
    const portPart = serverState ? ` · :${serverState.port}` : ''
    const title = suffix ? `${suffix} ${base}${portPart}` : `${base}${portPart}`
    process.stdout.write(`\x1b]0;${title}\x07`)
  }

  function startSpinner() {
    if (spinnerTimer) return
    spinnerTimer = setInterval(() => {
      spinnerState = advanceSpinner(spinnerState)
      // Terminal title animation
      const glyphs = ['·', '•', '·']
      const idx = spinnerState.frame % glyphs.length
      setTerminalTitle(glyphs[idx])
      renderStatus()
    }, SPINNER_INTERVAL_MS)
  }

  function stopSpinner() {
    if (spinnerTimer) {
      clearInterval(spinnerTimer)
      spinnerTimer = null
    }
    setTerminalTitle()
  }

  async function resumeSession(session: SessionMeta) {
    try {
      const transcript = await agent.loadTranscript(session.session_id)
      sessionId = session.session_id
      appState = { ...appState, sessionId: session.session_id }
      const { messagesToOutputLines } = await import('../render/output.js')
      const { transcriptToMessages } = await import('../session/transcript.js')
      const messages = transcriptToMessages(transcript as any)
      commitLines(messagesToOutputLines(messages))
      commitLines([
        { id: 'sys-resumed-gap', kind: 'system', text: '' },
        { id: 'sys-resumed', kind: 'system', text: chalk.dim(`  resumed session ${session.session_id.slice(0, 8)}`) },
      ])
    } catch (err: any) {
      commitLines([{ id: 'sys-err', kind: 'error', text: `Failed to resume: ${err?.message ?? err}` }])
    }
  }

  /** Get expanded text (paste refs resolved, image refs stripped). */
  function getExpandedText(): string {
    const raw = getEditorText(editor)
    const expanded = expandPasteRefs(raw, pastedChunks)
    return stripImageRefs(expanded).trim()
  }

  /** Get display text (raw with refs intact). */
  function getDisplayText(): string {
    return getEditorText(editor).trim()
  }

  /** Clear editor and paste state. */
  function clearAll() {
    editor = clearEditor(editor)
    pastedChunks.clear()
    pastedImages.clear()
  }

  /** Insert pasted text, collapsing large pastes into refs. */
  function insertPaste(raw: string) {
    const cleaned = cleanPastedText(raw)
    if (shouldCollapse(cleaned)) {
      const id = nextPasteId++
      const numLines = (cleaned.match(/\n/g) || []).length
      pastedChunks.set(id, cleaned)
      const ref = formatPastedTextRef(id, numLines)
      editor = insertText(editor, ref)
    } else {
      editor = insertText(editor, cleaned)
    }
  }

  /** Try to paste image from clipboard (Ctrl+V). */
  async function tryPasteImage() {
    const img = await getImageFromClipboard()
    if (img) {
      const id = nextPasteId++
      pastedImages.set(id, { id, base64: img.base64, mediaType: img.mediaType })
      editor = insertText(editor, formatImageRef(id))
      renderStatus()
    }
  }

  /** Build content blocks for images. */
  function buildImageContentBlocks(): ContentBlock[] | null {
    const displayText = getDisplayText()
    const imageRefs = parsePasteRefs(displayText).filter(r => r.type === 'image')
    const images = imageRefs
      .map(r => pastedImages.get(r.id))
      .filter((img): img is { id: number; base64: string; mediaType: string } => img !== undefined)
    if (images.length === 0) return null
    const blocks: ContentBlock[] = []
    const text = getExpandedText()
    if (text) blocks.push({ type: 'text', text })
    for (const img of images) {
      blocks.push({ type: 'image', data: img.base64, mimeType: img.mediaType })
    }
    return blocks
  }

  async function runQuery(text: string, contentJson?: string) {
    isLoading = true
    spinnerState = createSpinnerState()
    streamMachine = createStreamMachineState(appState, spinnerState)
    startSpinner()
    renderStatus()

    try {
      const stream = await agent.query(text, sessionId ?? undefined, planning ? 'planning_interactive' : 'interactive', contentJson)
      streamRef = stream
      sessionId = stream.sessionId ?? sessionId
      appState = { ...appState, sessionId: sessionId }
      screenLog.bind(stream.sessionId)

      for await (const event of stream) {
        if (destroyed) break
        if (!streamMachine) break

        if (event.kind === 'ask_user') {
          const payload = (event.payload ?? {}) as { questions?: AskUserRequest['questions'] }
          if (payload.questions && payload.questions.length > 0) {
            const questions: AskQuestion[] = payload.questions.map(q => ({
              header: q.header,
              question: q.question,
              options: q.options.map(o => ({ label: o.label, description: o.description })),
            }))
            overlay = { kind: 'ask-user', state: createAskState(questions) }
            // Append a single prompt line to scroll zone so the question
            // appears inline with message history; the actual interactive UI
            // renders in the status area and updates in-place.
            renderer.appendScroll(chalk.dim('  Agent is asking…'))
            renderStatus()
          }
          continue
        }

        const update = reduceRunEvent(streamMachine!, event, { termRows: renderer.termRows })

        streamMachine = update.state
        appState = update.state.appState
        spinnerState = update.state.spinnerState

        if (!update.suppressToolStarted && event.kind === 'tool_started') {
          commitLines(buildToolStartedLines(event))
          lastProgressLineCount = 0
        }
        if (!update.suppressToolFinished && event.kind === 'tool_finished') {
          commitToolFinished(event)
          lastProgressLineCount = 0
        }

        // In expanded mode, commit tool progress lines to scroll area
        if (expanded && event.kind === 'tool_progress') {
          const text = ((event.payload ?? {}) as Record<string, any>).text as string | undefined
          if (text) {
            const allLines = text.split('\n')
            const newLines = allLines.slice(lastProgressLineCount)
            lastProgressLineCount = allLines.length
            if (newLines.length > 0) {
              const outputLines: OutputLine[] = newLines.map(l => ({
                id: `prog-${Date.now()}`,
                kind: 'tool_result' as const,
                text: `  ${l}`,
              }))
              // Commit to both arrays but only render to scroll
              compactLines.push(...outputLines)
              expandedLines.push(...outputLines)
              const blocks = buildOutputBlocks(outputLines)
              renderer.beginBatch()
              renderer.appendScroll(blocksToLines(blocks).join('\n'))
              renderStatus()
              renderer.flushBatch()
              screenLog.log(outputLines)
            }
          }
        }

        if (update.commitLines.length > 0) {
          commitLines(update.commitLines)
        }

        if (update.rerenderStatus) renderStatus()
      }

      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        if (final.lines.length > 0) {
          commitLines(final.lines)
        }
        streamMachine = final.state
        appState = final.state.appState
      }
    } catch (err: any) {
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        if (final.lines.length > 0) commitLines(final.lines)
      }
      commitLines([{ id: 'sys-err', kind: 'error', text: err?.message ?? String(err) }])
    } finally {
      streamRef = null
      isLoading = false
      streamMachine = null
      stopSpinner()
      renderStatus()
    }
  }

  function handleKey(event: KeyEvent) {
    if (event.type === 'ctrl' && event.key === 'c') {
      if (isLoading && streamRef) {
        streamRef.abort()
        streamRef = null
        isLoading = false
        streamMachine = null
        stopSpinner()
        commitLines([{ id: 'sys-int', kind: 'system', text: '  Interrupted.' }])
        renderStatus()
      } else if (isEditorEmpty(editor)) {
        if (exitHint) {
          cleanup()
          if (sessionId) {
            process.stdout.write(`\n\x1b[90m${'─'.repeat(80)}\x1b[0m\n`)
            process.stdout.write(`\x1b[90mResume: evot --resume ${sessionId}\x1b[0m\n\n`)
          }
          process.exit(0)
        } else {
          exitHint = true
          renderStatus()
          if (exitHintTimer) clearTimeout(exitHintTimer)
          exitHintTimer = setTimeout(() => { exitHint = false; renderStatus() }, 2000)
        }
      } else {
        editor = clearEditor(editor)
        renderStatus()
      }
      return
    }

    // Any non-Ctrl+C input cancels exit hint
    if (exitHint && !(event.type === 'ctrl' && event.key === 'c')) {
      exitHint = false
    }

    if (event.type === 'escape') {
      if (overlay.kind !== 'none') {
        if (overlay.kind === 'ask-user' && streamRef) {
          streamRef.abort()
          streamRef = null
          overlay = { kind: 'none' }
          isLoading = false
          streamMachine = null
          stopSpinner()
          commitLines([{ id: 'sys-ask-cancel', kind: 'system', text: '  ⏺ Cancelled.' }])
          renderStatus()
          return
        }
        overlay = { kind: 'none' }
        renderStatus()
        return
      }
      if (isLoading && streamRef) {
        streamRef.abort()
        streamRef = null
        isLoading = false
        streamMachine = null
        stopSpinner()
        commitLines([{ id: 'sys-int', kind: 'system', text: '  Interrupted.' }])
        renderStatus()
      } else if (!isEditorEmpty(editor)) {
        editor = clearEditor(editor)
        renderStatus()
      }
      return
    }

    if (overlay.kind === 'help') {
      overlay = { kind: 'none' }
      renderStatus()
      return
    }

    if (overlay.kind === 'selector') {
      handleSelectorKey(event)
      return
    }

    if (overlay.kind === 'ask-user') {
      handleAskKey(event)
      return
    }

    if (isLoading) {
      if (event.type === 'enter') {
        const expandedText = getExpandedText()
        const displayText = getDisplayText()
        const imageBlocks = buildImageContentBlocks()

        // /log (no args) works during execution — show screen log path
        const trimmed = (expandedText || '').trim()
        if (trimmed === '/log') {
          clearAll()
          const logPath = screenLog.filePath
          if (logPath) commitLines([{ id: 'sys-log', kind: 'system', text: `  Log: ${logPath}` }])
          else commitLines([{ id: 'sys-log', kind: 'system', text: '  No active screen log.' }])
          renderStatus()
          return
        }

        // Allow text, images, or both
        if ((expandedText || imageBlocks) && streamRef) {
          if (imageBlocks) {
            // Steer with images (as content blocks)
            const contentJson = JSON.stringify(imageBlocks)
            streamRef.steer('', contentJson)
          } else {
            // Steer with text only
            streamRef.steer(expandedText)
          }
          commitLines(buildUserMessage(displayText))
          clearAll()
          renderStatus()
        }
        return
      } else if (event.type === 'char') {
        editor = insertText(editor, event.char)
        renderStatus()
        return
      } else if (event.type === 'paste') {
        insertPaste(event.text)
        renderStatus()
        return
      }
      // Fall through to normal key handling (backspace, arrows, etc.)
    }

    // --- Ctrl shortcuts ---
    if (event.type === 'ctrl') {
      switch (event.key) {
        case 'u':
          editor = clearLineBefore(editor)
          renderStatus()
          return
        case 'k':
          editor = clearLineAfter(editor)
          renderStatus()
          return
        case 'd':
          if (isEditorEmpty(editor)) {
            cleanup()
            process.exit(0)
          }
          editor = deleteForward(editor)
          renderStatus()
          return
        case 'w':
          editor = deleteWordBefore(editor)
          renderStatus()
          return
        case 'a':
          editor = moveHome(editor)
          renderStatus()
          return
        case 'e':
          editor = moveEnd(editor)
          renderStatus()
          return
        case 'l':
          clearAll()
          renderStatus()
          return
        case 'v':
          tryPasteImage()
          return
        case 'o':
          toggleExpanded()
          return
        default:
          return
      }
    }

    switch (event.type) {
      case 'enter': {
        const rawText = getEditorText(editor).trim()
        if (!rawText) return
        // Check for continuation (unclosed fences, trailing backslash)
        if (editorNeedsContinuation(editor)) {
          editor = insertNewline(editor)
          renderStatus()
          return
        }
        const expandedText = getExpandedText()
        const displayText = getDisplayText()
        const imageBlocks = buildImageContentBlocks()
        // Allow image-only submissions
        if (!expandedText && !imageBlocks) return
        clearAll()
        renderStatus()
        if (isSlashCommand(expandedText || rawText)) {
          handleSlashInput(expandedText || rawText)
        } else if (logMode) {
          // In log mode, send to forked agent
          const historyText = stripImageRefs(displayText)
          if (historyText) {
            historyMgr.append(historyText)
            historyState = pushHistory(historyState, historyText)
          }
          runLogQuery(logMode, expandedText)
        } else {
          // Save to history (strip image refs)
          const historyText = stripImageRefs(displayText)
          if (historyText) {
            historyMgr.append(historyText)
            historyState = pushHistory(historyState, historyText)
          }
          commitLines(buildUserMessage(displayText))
          if (imageBlocks) {
            const contentJson = JSON.stringify(imageBlocks)
            runQuery('', contentJson)
          } else {
            runQuery(expandedText)
          }
        }
        break
      }
      case 'alt-enter': {
        editor = insertNewline(editor)
        renderStatus()
        break
      }
      case 'tab': {
        const result = applyCompletion(editor)
        if (result.applied) {
          editor = result.state
          renderStatus()
        }
        break
      }
      case 'char':
        editor = insertText(editor, event.char)
        editor = refreshGhostHint(editor)
        renderStatus()
        break
      case 'paste':
        insertPaste(event.text)
        renderStatus()
        break
      case 'backspace': {
        // Check if we should delete an entire paste ref
        const currentLine = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(currentLine)
        const refDel = deleteRefBackspace(currentLine, editor.cursorCol, refs)
        if (refDel) {
          const deletedRef = refs.find(r => r.end === editor.cursorCol)
          if (deletedRef) {
            pastedChunks.delete(deletedRef.id)
            pastedImages.delete(deletedRef.id)
          }
          const newLines = [...editor.lines]
          newLines[editor.cursorLine] = refDel.newLine
          editor = { ...editor, lines: newLines, cursorCol: refDel.newCursorCol, ghostHint: '', completionCandidates: [] }
        } else {
          editor = backspace(editor)
        }
        editor = refreshGhostHint(editor)
        renderStatus()
        break
      }
      case 'left': {
        const line = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(line)
        const skip = skipRefOnMove(editor.cursorCol, 'left', refs)
        if (skip !== null) {
          editor = { ...editor, cursorCol: skip, ghostHint: '' }
        } else {
          editor = moveLeft(editor)
        }
        renderStatus()
        break
      }
      case 'right': {
        const line = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(line)
        const skip = skipRefOnMove(editor.cursorCol, 'right', refs)
        if (skip !== null) {
          editor = { ...editor, cursorCol: skip, ghostHint: '' }
        } else {
          editor = moveRight(editor)
        }
        renderStatus()
        break
      }
      case 'home':
        editor = moveHome(editor)
        renderStatus()
        break
      case 'end':
        editor = moveEnd(editor)
        renderStatus()
        break
      case 'up': {
        // Multi-line: move cursor up within editor
        if (editor.lines.length > 1) {
          editor = moveUp(editor)
          renderStatus()
          break
        }
        // Single-line: history navigation
        const result = historyPrev(historyState, editor)
        if (result.changed) {
          historyState = result.history
          editor = result.editor
          renderStatus()
        }
        break
      }
      case 'down': {
        // Multi-line: move cursor down within editor
        if (editor.lines.length > 1) {
          editor = moveDown(editor)
          renderStatus()
          break
        }
        // Single-line: history navigation
        const result = historyNext(historyState, editor)
        if (result.changed) {
          historyState = result.history
          editor = result.editor
          renderStatus()
        }
        break
      }
      default:
        break
    }
  }

  async function handleSlashInput(text: string) {
    const result = handleSlashCommand(text, {
      agent,
      appState,
      configInfo,
      preloadedSessions,
      planning,
    })
    appState = result.appState
    planning = result.planning
    if (result.overlay) overlay = result.overlay
    if (result.clearScreen) process.stdout.write('\x1b[2J\x1b[H')
    if (result.exit) { cleanup(); process.exit(0) }
    if (result.resumeSession) await resumeSession(result.resumeSession)
    if (result.systemLines.length > 0) commitLines(result.systemLines)

    // Handle async commands that the simple handleSlashCommand can't do
    const resolved = resolveCommand(text)
    if (resolved.kind !== 'resolved') {
      renderStatus()
      return
    }
    const { name, args } = resolved

    if (name === '/new') {
      if (isLoading && streamRef) { streamRef.abort(); streamRef = null; isLoading = false; streamMachine = null; stopSpinner() }
      sessionId = null
      appState = { ...createInitialState(appState.model, agent.cwd), verbose: appState.verbose }
      commitLines([{ id: 'sys-new', kind: 'system', text: '  New session started.' }])
    } else if (name === '/compact') {
      try {
        const outcome = await agent.submit('/clear', sessionId ?? undefined)
        if (outcome.kind === 'command') {
          commitLines([{ id: 'sys-compact', kind: 'system', text: `  ${outcome.message}` }])
        }
      } catch (err: any) {
        commitLines([{ id: 'sys-compact-err', kind: 'system', text: chalk.red(`  Compact failed: ${err?.message ?? err}`) }])
      }
    } else if (name === '/goto') {
      if (!args) {
        commitLines([{ id: 'sys-goto', kind: 'system', text: '  Usage: /goto <message_number>' }])
      } else {
        try {
          const outcome = await agent.submit(`/goto ${args}`, sessionId ?? undefined)
          if (outcome.kind === 'command') {
            commitLines([{ id: 'sys-goto', kind: 'system', text: `  ${outcome.message}` }])
          }
        } catch (err: any) {
          commitLines([{ id: 'sys-goto-err', kind: 'system', text: chalk.red(`  Goto failed: ${err?.message ?? err}`) }])
        }
      }
    } else if (name === '/history') {
      try {
        const histCmd = args ? `/history ${args}` : '/history'
        const outcome = await agent.submit(histCmd, sessionId ?? undefined)
        if (outcome.kind === 'command') {
          commitLines([{ id: 'sys-hist', kind: 'system', text: `  ${outcome.message}` }])
        }
      } catch (err: any) {
        commitLines([{ id: 'sys-hist-err', kind: 'system', text: chalk.red(`  History failed: ${err?.message ?? err}`) }])
      }
    } else if (name === '/env') {
      handleEnvCommand(args)
    } else if (name === '/skill') {
      await handleSkillCommand(args)
    } else if (name === '/update') {
      await handleUpdateCommand()
    } else if (name === '/act' || name === '/done') {
      if (logMode) {
        logMode = null
        commitLines([{ id: 'sys-log-exit', kind: 'system', text: '  [log mode] exited' }])
      } else {
        planning = false
        commitLines([{ id: 'sys-act', kind: 'system', text: '  planning: off' }])
      }
    } else if (name === '/log') {
      await handleLogCommand(args)
    } else if (name === '/resume') {
      // Load sessions (default 20; fetch all only when searching by id)
      try {
        const allSessions: SessionMeta[] = await agent.listSessions(args ? 0 : 20)
        if (args) {
          // Resume specific session by id prefix
          const match = allSessions.find(
            (s) => s.session_id === args || s.session_id.startsWith(args)
          )
          if (match) {
            await resumeSession(match)
          } else {
            commitLines([{ id: 'sys-r', kind: 'system', text: chalk.red(`  Session not found: ${args}`) }])
          }
        } else {
          // Prefer sessions from current project, fall back to all
          const cwdSessions = allSessions.filter(s => s.cwd === agent.cwd)
          const sessions = cwdSessions.length > 0 ? cwdSessions : allSessions
          if (sessions.length === 0) {
            commitLines([{ id: 'sys-r', kind: 'system', text: '  No sessions found' }])
          } else {
            // Load all sessions for search scope, but display only first 20
            const displaySessions = sessions.slice(0, 20)
            const allForSearch = await agent.listSessions(0)
            const searchSessions = allForSearch.filter(s => s.cwd === agent.cwd).length > 0
              ? allForSearch.filter(s => s.cwd === agent.cwd)
              : allForSearch
            overlay = {
              kind: 'selector',
              state: createSelectorState('Resume session', displaySessions.map(s => {
                const title = s.title || '(untitled)'
                const short = title.length > 50 ? title.slice(0, 49) + '…' : title
                const tag = s.source ? `[${s.source}] ` : ''
                const time = relativeTime(s.updated_at)
                return { label: s.session_id.slice(0, 8), detail: `${tag}${short}  ${time}` }
              }), searchSessions.map(s => {
                const title = s.title || '(untitled)'
                const short = title.length > 50 ? title.slice(0, 49) + '…' : title
                const tag = s.source ? `[${s.source}] ` : ''
                const time = relativeTime(s.updated_at)
                return { label: s.session_id.slice(0, 8), detail: `${tag}${short}  ${time}` }
              })),
            }
          }
        }
      } catch (err: any) {
        commitLines([{ id: 'sys-r-err', kind: 'system', text: chalk.red(`  Failed to list sessions: ${err?.message ?? err}`) }])
      }
    } else if (name === '/model' && !args) {
      // Show model selector overlay
      const models = configInfo?.availableModels ?? [agent.model]
      if (models.length > 1) {
        overlay = {
          kind: 'selector',
          state: createSelectorState('Select model', models.map(m => ({
            label: m,
            detail: m === agent.model ? '(current)' : undefined,
          }))),
        }
      } else {
        commitLines([{ id: 'sys-m', kind: 'system', text: '  Only one model available.' }])
      }
    }

    renderStatus()
  }

  function handleEnvCommand(args: string) {
    const sub = args.trim()
    if (!sub) {
      const vars = agent.listVariables()
      if (vars.length === 0) {
        commitLines([{ id: 'sys-env', kind: 'system', text: '  No variables set' }])
      } else {
        for (const v of vars) {
          commitLines([{ id: `sys-env-${v.key}`, kind: 'system', text: `  ${v.key}=${v.value}` }])
        }
      }
    } else if (sub.startsWith('set ')) {
      const eq = sub.slice(4).trim()
      const eqIdx = eq.indexOf('=')
      if (eqIdx <= 0) {
        commitLines([{ id: 'sys-env-err', kind: 'system', text: '  Usage: /env set KEY=VALUE' }])
      } else {
        const key = eq.slice(0, eqIdx)
        const value = eq.slice(eqIdx + 1)
        agent.setVariable(key, value)
        commitLines([{ id: 'sys-env-set', kind: 'system', text: `  ${key}=${value}` }])
      }
    } else if (sub.startsWith('del ')) {
      const key = sub.slice(4).trim()
      agent.deleteVariable(key)
      commitLines([{ id: 'sys-env-del', kind: 'system', text: `  deleted: ${key}` }])
    } else {
      commitLines([{ id: 'sys-env-err', kind: 'system', text: '  Usage: /env [set K=V | del K]' }])
    }
  }

  async function handleSkillCommand(args: string) {
    const sub = args.trim()
    if (!sub || sub === 'list') {
      try {
        const { skillList } = await import('../commands/skill.js')
        commitLines([{ id: 'sys-skill', kind: 'system', text: skillList() }])
      } catch {
        commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  skill list unavailable' }])
      }
    } else if (sub.startsWith('install ')) {
      const source = sub.slice(8).trim()
      if (!source) {
        commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  Usage: /skill install <owner/repo>' }])
      } else {
        commitLines([{ id: 'sys-skill-inst', kind: 'system', text: `  cloning ${source}` }])
        renderStatus()
        try {
          const { skillInstall } = await import('../commands/skill.js')
          const forked = agent.fork('You analyze skills and provide setup guides.')
          const result = await skillInstall(source, forked, (msg, level) => {
            commitLines([{ id: `sys-skill-${Date.now()}`, kind: 'system', text: `  ${msg}` }])
            renderStatus()
          })
          if (result) commitLines([{ id: 'sys-skill-done', kind: 'system', text: `  ${result}` }])
        } catch (err: any) {
          commitLines([{ id: 'sys-skill-err', kind: 'system', text: chalk.red(`  install failed: ${err?.message ?? err}`) }])
        }
      }
    } else if (sub.startsWith('remove ')) {
      const name = sub.slice(7).trim()
      if (!name) {
        commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  Usage: /skill remove <name>' }])
      } else {
        try {
          const { skillRemove } = await import('../commands/skill.js')
          commitLines([{ id: 'sys-skill-rm', kind: 'system', text: skillRemove(name) }])
        } catch {
          commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  skill remove unavailable' }])
        }
      }
    } else {
      commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  Usage: /skill [list | install <source> | remove <name>]' }])
    }
    renderStatus()
  }

  async function handleUpdateCommand() {
    commitLines([{ id: 'sys-upd', kind: 'system', text: '  checking for updates...' }])
    renderStatus()
    try {
      const { runUpdate } = await import('../update/index.js')
      const { version } = await import('../native/index.js')
      const result = await runUpdate(version())
      switch (result.kind) {
        case 'up_to_date':
          commitLines([{ id: 'sys-upd-ok', kind: 'system', text: '  ✓ evot is up to date.' }])
          break
        case 'updated': {
          const lines: string[] = [`  ✓ updated ${result.from} → ${result.to}. restart evot to apply.`]
          if (result.notes && result.notes.length > 0) {
            lines.push('')
            lines.push(`  What's new in ${result.to}:`)
            for (const note of result.notes) {
              lines.push(`    • ${note}`)
            }
          }
          commitLines([{ id: 'sys-upd-ok', kind: 'system', text: lines.join('\n') }])
          break
        }
        case 'error':
          commitLines([{ id: 'sys-upd-err', kind: 'system', text: chalk.red(`  ✗ ${result.message}`) }])
          break
      }
    } catch (err: any) {
      commitLines([{ id: 'sys-upd-err', kind: 'system', text: chalk.red(`  ✗ update failed: ${err?.message ?? err}`) }])
    }
  }

  async function handleLogCommand(args: string) {
    const query = args.trim()
    const { join } = await import('path')
    const { homedir } = await import('os')
    const logDir = join(homedir(), '.evotai', 'logs')
    const sid = sessionId

    if (query.startsWith('up')) {
      // /log up [session_id] — upload/share session
      const upArg = query.slice(2).trim()
      let resolvedSid = upArg || sid
      if (!resolvedSid) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: '  No active session to upload.' }])
        return
      }
      if (upArg && upArg.length < 36) {
        try {
          const sessions = await agent.listSessions(20)
          const matches = sessions.filter(s => s.session_id.startsWith(upArg))
          if (matches.length === 0) {
            commitLines([{ id: 'sys-log-err', kind: 'system', text: `  Session not found: ${upArg}` }])
            return
          }
          if (matches.length > 1) {
            commitLines([{ id: 'sys-log-err', kind: 'system', text: `  Ambiguous session id: ${upArg} (${matches.length} matches)` }])
            return
          }
          resolvedSid = matches[0]!.session_id
        } catch (err: any) {
          commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Failed to list sessions: ${err?.message ?? err}`) }])
          return
        }
      }
      commitLines([{ id: 'sys-log-up', kind: 'system', text: `  packing session ${resolvedSid!.slice(0, 8)}...` }])
      renderStatus()
      try {
        const { logPut } = await import('../commands/log-share.js')
        const result = await logPut(resolvedSid!)
        commitLines([{ id: 'sys-log-url', kind: 'system', text: `  uploaded. share this link:\n  ${result.url}\n  ⏳ link expires in 60 minutes` }])
      } catch (err: any) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Export failed: ${err?.message ?? err}`) }])
      }
    } else if (query.startsWith('dl ')) {
      // /log dl <url#password>
      const dlUrl = query.slice(3).trim()
      if (!dlUrl) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: '  Usage: /log dl <url#password>' }])
        return
      }
      commitLines([{ id: 'sys-log-dl', kind: 'system', text: '  downloading and importing...' }])
      renderStatus()
      try {
        const { logGet } = await import('../commands/log-share.js')
        const result = await logGet(dlUrl)
        commitLines([{ id: 'sys-log-dl-ok', kind: 'system', text: `  imported session: ${result.sessionId}\n  resume with: /resume ${result.sessionId.slice(0, 8)}` }])
      } catch (err: any) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Import failed: ${err?.message ?? err}`) }])
      }
    } else if (!query) {
      const logPath = screenLog.filePath
      if (logPath) commitLines([{ id: 'sys-log', kind: 'system', text: `  Log: ${logPath}` }])
      else if (sid) commitLines([{ id: 'sys-log', kind: 'system', text: `  Log: ${join(logDir, `${sid}.screen.log`)}` }])
      else commitLines([{ id: 'sys-log', kind: 'system', text: `  Log dir: ${logDir} (no active session)` }])
    } else if (!sid) {
      commitLines([{ id: 'sys-log-err', kind: 'system', text: '  No active session to analyze.' }])
    } else {
      // /log <query> — fork agent to analyze log
      const logPath = join(logDir, `${sid}.screen.log`)
      const systemPrompt = [
        'You are in a temporary log analysis session.',
        'This session is not persisted and does not affect the main session context.',
        '',
        `Log file to analyze:\n${logPath}`,
        '',
        'Rules:',
        '- Read relevant log sections before answering; do not guess',
        '- Prefer partial reads; avoid loading the entire file at once',
        '- Use search to locate key information when needed',
        '- Do not modify any files',
      ].join('\n')
      try {
        const forked = agent.fork(systemPrompt)
        logMode = forked
        commitLines([{ id: 'sys-log-mode', kind: 'system', text: `  [log mode] analyzing: ${logPath}\n  not persisted. type /done to return.` }])
        renderStatus()
        await runLogQuery(forked, query)
      } catch (err: any) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Fork failed: ${err?.message ?? err}`) }])
      }
    }
    renderStatus()
  }

  async function runLogQuery(forked: import('../native/index.js').ForkedAgent, prompt: string) {
    isLoading = true
    spinnerState = createSpinnerState()
    startSpinner()
    renderStatus()
    commitLines(buildUserMessage(prompt))

    try {
      const stream = await forked.query(prompt)
      streamRef = stream
      let streamingText = ''

      for await (const event of stream) {
        if (destroyed) break
        if (event.kind === 'assistant_delta') {
          const delta = (event.payload as any)?.delta as string | undefined
          if (delta) {
            streamingText += delta
            const lastNl = streamingText.lastIndexOf('\n')
            if (lastNl >= 0) {
              const complete = streamingText.slice(0, lastNl + 1)
              streamingText = streamingText.slice(lastNl + 1)
              if (complete.trim()) {
                commitLines(buildAssistantLines(complete))
              }
            }
          }
        } else if (event.kind === 'tool_started') {
          if (streamingText.trim()) { commitLines(buildAssistantLines(streamingText)); streamingText = '' }
          commitLines(buildToolStartedLines(event))
        } else if (event.kind === 'tool_finished') {
          commitToolFinished(event)
        }
      }
      if (streamingText.trim()) commitLines(buildAssistantLines(streamingText))
    } catch (err: any) {
      commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Log query failed: ${err?.message ?? err}`) }])
    } finally {
      streamRef = null
      isLoading = false
      stopSpinner()
      renderStatus()
    }
  }

  function handleSelectorKey(event: KeyEvent) {
    if (overlay.kind !== 'selector') return
    let state = overlay.state

    switch (event.type) {
      case 'up':
        state = selectorUp(state)
        break
      case 'down':
        state = selectorDown(state)
        break
      case 'char':
        state = selectorType(state, event.char)
        break
      case 'backspace':
        state = selectorBackspace(state)
        break
      case 'enter': {
        const selected = selectorSelect(state)
        overlay = { kind: 'none' }
        if (selected) {
          if (state.title === 'Resume session') {
            // Resume selector result — find session by id prefix
            const match = preloadedSessions.find(s => s.session_id.startsWith(selected.label))
            if (match) resumeSession(match)
          } else {
            // Model selector result
            agent.model = selected.label
            syncProvider(agent, selected.label, configInfo)
            appState = { ...appState, model: selected.label }
            commitLines([{ id: 'sys-model', kind: 'system', text: `  Model → ${selected.label}` }])
          }
        }
        renderStatus()
        return
      }
      case 'escape':
        overlay = { kind: 'none' }
        renderStatus()
        return
      default:
        break
    }

    overlay = { kind: 'selector', state }
    renderStatus()
  }

  function handleAskKey(event: KeyEvent) {
    if (overlay.kind !== 'ask-user') return

    const result = handleAskKeyEvent(overlay.state, event.type, event.type === 'char' ? event.char : undefined)

    switch (result.action) {
      case 'cancel':
        if (streamRef) {
          streamRef.abort()
          streamRef = null
        }
        overlay = { kind: 'none' }
        isLoading = false
        streamMachine = null
        stopSpinner()
        commitLines([{ id: 'sys-ask-cancel', kind: 'system', text: '  ⏺ Cancelled.' }])
        renderStatus()
        return
      case 'submit':
        if (streamRef) {
          const response = askStateToResponse(result.state)
          streamRef.respondAskUser(JSON.stringify({ Answered: response }))
          overlay = { kind: 'none' }
          const answerLines: OutputLine[] = response.map((r, i) => ({
            id: `sys-ask-${i}`,
            kind: 'system' as const,
            text: `  ● ${r.header}: ${r.answer}`,
          }))
          commitLines(answerLines)
        }
        renderStatus()
        return
      case 'update':
        overlay = { kind: 'ask-user', state: result.state }
        renderStatus()
        return
    }
  }

  const disableRaw = enableRawMode(process.stdin)
  const { stream: pasteStream, cleanup: cleanupPaste } = installBracketedPaste(process.stdin, () => {
    // Empty paste — likely Cmd+V with image in clipboard
    tryPasteImage()
  })
  pasteStream.on('data', (data: Buffer) => {
    const events = parseInput(data)
    for (const ev of events) handleKey(ev)
  })

  process.stdout.write('\x1b[?2004h')
  renderStatus()

  function cleanup() {
    destroyed = true
    stopSpinner()
    updateMgr.cleanup()
    if (exitHintTimer) clearTimeout(exitHintTimer)
    process.stdout.write('\x1b[?2004l')
    setTerminalTitle()
    cleanupPaste()
    disableRaw()
    renderer.destroy()
  }

  process.on('SIGINT', cleanup)
  process.on('SIGTERM', cleanup)

  await new Promise<void>(() => {})
}
