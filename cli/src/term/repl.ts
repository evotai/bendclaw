import { TermRenderer } from './renderer.js'
import { parseInput, enableRawMode, type KeyEvent } from './input.js'
import { installBracketedPaste } from './bracketed-paste.js'
import { createSpinnerState, advanceSpinner } from './spinner.js'
import { createSelectorState, selectorExpandItems, selectorClearQuery } from './selector.js'
import { createAskState, handleAskKeyEvent, type AskQuestion } from './ask.js'
import { buildUserMessage, buildAssistantLines, type OutputLine } from '../render/output.js'
import { Agent, QueryStream, type SessionMeta, type ConfigInfo } from '../native/index.js'
import { createInitialState, type AppState } from './app/state.js'
import type { AskUserRequest } from './app/types.js'
import { HistoryManager, parseHistoryItems } from '../session/history.js'
import { ScreenLog } from '../session/screen-log.js'
import { isSlashCommand, resolveCommand } from '../commands/index.js'
import { renderBanner } from './banner.js'
import {
  buildOutputBlocks,
  buildActiveResponseBlocks,
  buildPromptBlocks,
  buildOverlayBlocks,
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
  buildToolProgressLines,
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
  stripImageRefs,
  deleteRefBackspace,
  skipRefOnMove,
  resolveSubmitText,
} from './input/paste_refs.js'
import { getImageFromClipboard } from './input/clipboard_image.js'
import { storeImage, formatImageSourceText } from './input/image_store.js'
import type { ContentBlock } from '../native/index.js'
import { tryStartServer, formatUptime, type ServerState } from './app/server.js'
import {
  RESUME_SELECTOR_TITLE,
  formatSessionItems,
  formatSessionWithTextItems,
  isResumeSelectorTitle,
  isSessionIdPrefix,
  resolveSessionByPrefix,
  selectSessionPool,
} from './app/resume.js'
import { chooseBannerSessions, findPreviousSession, previousSessionLine } from './app/session-view.js'
import { handleSelectorControl } from './app/selector-control.js'
import { decideReplControl, type ReplControlAction } from './app/repl-control.js'

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
  const pastedImages = new Map<number, { id: number; base64: string; mediaType: string; filePath?: string }>()
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

  const bannerText = currentBannerText()
  renderer.appendScroll(bannerText)
  setTerminalTitle('✳')

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
    const match = findPreviousSession(preloadedSessions, agent.cwd)
    if (match) commitLines([previousSessionLine(match)])
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

  function currentToolProgress(): string {
    return streamMachine?.toolProgress || streamMachine?.lastToolProgress || ''
  }

  function currentBannerText(): string {
    const sessions = chooseBannerSessions(preloadedSessions, agent.cwd)
    return renderBanner(agent.model, agent.cwd, configInfo, sessions, renderer.termCols, serverState)
  }

  function renderStatus() {
    if (destroyed) return
    const isAssistantStreaming = streamMachine?.spinnerState.streaming ?? false
    const pendingText = isAssistantStreaming ? '' : streamMachine?.pendingText ?? ''
    const toolProgress = currentToolProgress()

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

  function restoreCurrentViewport() {
    const lines = expanded ? expandedLines : compactLines
    const output = lines.length > 0 ? blocksToLines(buildOutputBlocks(lines)).join('\n') : ''
    const text = [currentBannerText().trimEnd(), output].filter(Boolean).join('\n')
    renderer.restoreViewport()
    if (text) renderer.appendScroll(text)
  }

  function commitLines(outputLines: OutputLine[]) {
    if (outputLines.length === 0) return
    const prevKind = compactLines.length > 0 ? compactLines[compactLines.length - 1]!.kind : undefined
    compactLines.push(...outputLines)
    expandedLines.push(...outputLines)
    const visible = expanded ? expandedLines.slice(-outputLines.length) : outputLines
    const blocks = buildOutputBlocks(visible, prevKind)
    const rendered = blocksToLines(blocks)
    renderer.beginBatch()
    renderer.appendScroll(rendered.join('\n'))
    renderStatus()
    renderer.flushBatch()
    screenLog.logLines(rendered)
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
    const rendered = blocksToLines(blocks)
    renderer.beginBatch()
    renderer.appendScroll(rendered.join('\n'))
    renderStatus()
    renderer.flushBatch()
    screenLog.logLines(rendered)
  }

  /** Toggle expanded view and redraw. */
  function toggleExpanded(): void {
    expanded = !expanded
    const lines = expanded ? expandedLines : compactLines
    renderer.beginBatch()
    if (expanded) {
      const snapshot = currentToolProgress()
      const snapshotLines: OutputLine[] = snapshot
        ? snapshot.split('\n').map(l => ({
            id: `prog-snapshot-${Date.now()}`,
            kind: 'tool_result' as const,
            text: `  ${l}`,
          }))
        : []
      const viewLines = snapshotLines.length > 0 ? [...lines, ...snapshotLines] : lines
      renderer.redrawViewport(viewLines.length > 0 ? blocksToLines(buildOutputBlocks(viewLines)).join('\n') : '')
    } else {
      renderer.restoreViewport()
      if (lines.length > 0) {
        renderer.appendScroll(blocksToLines(buildOutputBlocks(lines)).join('\n'))
      }
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

  let titleFrame = 0
  const TITLE_INTERVAL_FRAMES = Math.round(960 / SPINNER_INTERVAL_MS) // ~960ms like Claude Code

  function startSpinner() {
    if (spinnerTimer) return
    titleFrame = 0
    spinnerTimer = setInterval(() => {
      spinnerState = advanceSpinner(spinnerState)
      if (streamMachine) {
        streamMachine = { ...streamMachine, spinnerState }
      }
      // Terminal title animation — update at ~960ms, not every spinner frame
      if (spinnerState.frame % TITLE_INTERVAL_FRAMES === 0) {
        const glyphs = ['⠂', '⠐']
        const idx = titleFrame % glyphs.length
        titleFrame++
        setTerminalTitle(glyphs[idx])
      }
      renderStatus()
    }, SPINNER_INTERVAL_MS)
  }

  function stopSpinner() {
    if (spinnerTimer) {
      clearInterval(spinnerTimer)
      spinnerTimer = null
    }
    setTerminalTitle('✳')
  }

  async function resumeSession(session: SessionMeta) {
    try {
      const transcript = await agent.loadTranscript(session.session_id)
      sessionId = session.session_id
      // Ensure we have the model — fetch from storage if missing
      let model = session.model
      if (!model) {
        const full = await agent.findSession(session.session_id)
        if (full?.model) model = full.model
      }
      if (model) {
        agent.model = model
      }
      appState = { ...appState, sessionId: session.session_id, model: model || appState.model }
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

  /** Get expanded text — resolves paste refs, strips only resolved image refs. */
  function getExpandedText(resolvedImageIds?: Set<number>): string {
    return resolveSubmitText(getEditorText(editor), pastedChunks, resolvedImageIds ?? null)
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
      // Store to disk immediately so images survive past session memory
      const filePath = await storeImage(img.base64, img.mediaType)
      pastedImages.set(id, { id, base64: img.base64, mediaType: img.mediaType, filePath: filePath ?? undefined })
      editor = insertText(editor, formatImageRef(id))
      renderStatus()
    }
  }

  /** Build content blocks for images. Returns blocks and resolved image IDs. */
  function buildImageContentBlocks(): { blocks: ContentBlock[]; resolvedIds: Set<number> } | null {
    const displayText = getDisplayText()
    const imageRefs = parsePasteRefs(displayText).filter(r => r.type === 'image')
    const resolved: { id: number; base64: string; mediaType: string; filePath?: string }[] = []
    const unresolvedIds = new Set<number>()
    for (const ref of imageRefs) {
      const img = pastedImages.get(ref.id)
      if (img) {
        resolved.push(img)
      } else {
        unresolvedIds.add(ref.id)
      }
    }
    if (resolved.length === 0) return null
    const blocks: ContentBlock[] = []
    // Only strip resolved image refs from text — unresolved ones stay as [Image #N]
    const text = getExpandedText(new Set(resolved.map(r => r.id)))
    // Annotate with image source paths so the model can reference files on disk
    const sourceAnnotations = resolved
      .filter(r => r.filePath)
      .map(r => formatImageSourceText(r.id, r.filePath!))
      .join('\n')
    const fullText = sourceAnnotations ? `${text}\n${sourceAnnotations}` : text
    if (fullText) blocks.push({ type: 'text', text: fullText })
    for (const img of resolved) {
      blocks.push({ type: 'image', data: img.base64, mimeType: img.mediaType })
    }
    return { blocks, resolvedIds: new Set(resolved.map(r => r.id)) }
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
            const baseline = Math.max(lastProgressLineCount, currentToolProgress().split('\n').length)
            const newLines = allLines.slice(baseline)
            lastProgressLineCount = allLines.length
            if (newLines.length > 0) {
              const compactProgress = buildToolProgressLines({ ...event, payload: { ...(event.payload ?? {}), text: newLines.join('\n') } })
              const expandedProgress = buildToolProgressLines({ ...event, payload: { ...(event.payload ?? {}), text: newLines.join('\n') } }, true)
              expandedLines.push(...expandedProgress)
              const blocks = buildOutputBlocks(expandedProgress)
              const rendered = blocksToLines(blocks)
              renderer.beginBatch()
              renderer.appendScroll(rendered.join('\n'))
              renderStatus()
              renderer.flushBatch()
              screenLog.logLines(rendered)
            }
          }
        }

        if (update.commitLines.length > 0) {
          if (update.expandedCommitLines) {
            // Dual-commit: compact in compactLines, expanded in expandedLines
            const compact = update.commitLines
            const exp = update.expandedCommitLines
            const prevKind = compactLines.length > 0 ? compactLines[compactLines.length - 1]!.kind : undefined
            compactLines.push(...compact)
            expandedLines.push(...exp)
            const visible = expanded ? exp : compact
            const blocks = buildOutputBlocks(visible, prevKind)
            const rendered = blocksToLines(blocks)
            renderer.beginBatch()
            renderer.appendScroll(rendered.join('\n'))
            renderStatus()
            renderer.flushBatch()
            screenLog.logLines(rendered)
          } else {
            commitLines(update.commitLines)
          }
        }

        if (update.rerenderStatus && !streamMachine?.spinnerState.streaming) renderStatus()
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
    const actions = decideReplControl({
      event,
      overlay,
      isLoading,
      hasStream: streamRef !== null,
      editor,
      exitHint,
      logMode: logMode !== null,
    })

    for (const action of actions) {
      if (applyReplControlAction(action, event)) return
    }
  }

  function applyReplControlAction(action: ReplControlAction, event: KeyEvent): boolean {
    switch (action.kind) {
      case 'interrupt':
        interruptStream('sys-int', '  Interrupted.')
        return true
      case 'exit':
        cleanup()
        if (sessionId) {
          process.stdout.write(`\n\x1b[90m${'─'.repeat(80)}\x1b[0m\n`)
          process.stdout.write(`\x1b[90mResume: evot --resume ${sessionId}\x1b[0m\n\n`)
        }
        process.exit(0)
      case 'show-exit-hint':
        exitHint = true
        renderStatus()
        if (exitHintTimer) clearTimeout(exitHintTimer)
        exitHintTimer = setTimeout(() => { exitHint = false; renderStatus() }, 2000)
        return true
      case 'clear-editor':
        editor = clearEditor(editor)
        renderStatus()
        return true
      case 'clear-exit-hint':
        exitHint = false
        return false
      case 'cancel-ask':
        overlay = { kind: 'none' }
        interruptStream('sys-ask-cancel', '  ⏺ Cancelled.')
        return true
      case 'clear-selector-query':
        if (overlay.kind === 'selector') overlay = { kind: 'selector', state: selectorClearQuery(overlay.state) }
        renderStatus()
        return true
      case 'close-overlay': {
        const redraw = overlay.kind === 'selector'
        overlay = { kind: 'none' }
        if (redraw) {
          renderer.beginBatch()
          restoreCurrentViewport()
          renderStatus()
          renderer.flushBatch()
        } else {
          renderStatus()
        }
        return true
      }
      case 'exit-log-mode':
        logMode = null
        commitLines([{ id: 'sys-log-exit', kind: 'system', text: '  [log mode] exited' }])
        renderStatus()
        return true
      case 'selector-key':
        handleSelectorKey(event)
        return true
      case 'ask-key':
        handleAskKey(event)
        return true
      case 'toggle-expanded':
        toggleExpanded()
        return true
      case 'loading-enter':
        handleLoadingEnter()
        return true
      case 'loading-char':
        if (event.type === 'char') {
          editor = insertText(editor, event.char)
          renderStatus()
        }
        return true
      case 'loading-paste':
        if (event.type === 'paste') {
          insertPaste(event.text)
          renderStatus()
        }
        return true
      case 'normal-key':
        handleNormalKey(event)
        return true
    }
  }

  /** Flush any in-progress content from the stream machine to the scroll area.
   *  Call before nulling streamMachine on any abort/cancel path. */
  function flushStreamContent() {
    if (!streamMachine) return
    const flushed = flushStreaming(streamMachine)
    if (flushed.lines.length > 0) commitLines(flushed.lines)
    // Preserve tool progress that was only shown in the status area
    const progress = streamMachine.lastToolProgress
    if (progress) {
      const toolName = streamMachine.spinnerState.toolName ?? 'bash'
      commitLines(buildToolProgressLines(
        { kind: 'tool_progress', payload: { tool_name: toolName, text: progress } } as any,
        expanded,
      ))
    }
  }

  function interruptStream(id: string, text: string) {
    if (streamRef) {
      streamRef.abort()
      streamRef = null
    }
    isLoading = false
    flushStreamContent()
    streamMachine = null
    stopSpinner()
    commitLines([{ id, kind: 'system', text }])
  }

  function handleLoadingEnter() {
    const displayText = getDisplayText()
    const imageResult = buildImageContentBlocks()
    const imageBlocks = imageResult?.blocks ?? null
    const expandedText = imageResult
      ? getExpandedText(imageResult.resolvedIds)
      : getExpandedText()

    const trimmed = (expandedText || '').trim()
    if (trimmed === '/log') {
      clearAll()
      const logPath = screenLog.filePath
      if (logPath) commitLines([{ id: 'sys-log', kind: 'system', text: `  Log: ${logPath}` }])
      else commitLines([{ id: 'sys-log', kind: 'system', text: '  No active screen log.' }])
      renderStatus()
      return
    }

    if ((expandedText || imageBlocks) && streamRef) {
      if (imageBlocks) {
        const contentJson = JSON.stringify(imageBlocks)
        streamRef.steer('', contentJson)
      } else {
        streamRef.steer(expandedText)
      }
      commitLines(buildUserMessage(displayText))
      clearAll()
      renderStatus()
    }
  }

  function handleNormalKey(event: KeyEvent) {
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
        const displayText = getDisplayText()
        const imageResult = buildImageContentBlocks()
        const imageBlocks = imageResult?.blocks ?? null
        // expandedText: only strip image refs that have resolved data.
        // Unresolved ones (e.g. from history) stay as [Image #N] text markers.
        const expandedText = imageResult
          ? getExpandedText(imageResult.resolvedIds)
          : getExpandedText()
        // Allow image-only or text-only submissions
        if (!expandedText && !imageBlocks) return
        clearAll()
        renderStatus()
        if (isSlashCommand(expandedText || rawText)) {
          if (displayText) {
            historyMgr.append(displayText)
            historyState = pushHistory(historyState, displayText)
          }
          handleSlashInput(expandedText || rawText)
        } else if (logMode) {
          // In log mode, send to forked agent
          if (displayText) {
            historyMgr.append(displayText)
            historyState = pushHistory(historyState, displayText)
          }
          runLogQuery(logMode, expandedText)
        } else {
          // Save to history
          if (displayText) {
            historyMgr.append(displayText)
            historyState = pushHistory(historyState, displayText)
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
    if (result.clearScreen) {
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
    }
    if (result.clearContext) {
      // Abort any in-flight streaming
      if (isLoading && streamRef) {
        streamRef.abort(); streamRef = null; isLoading = false
        flushStreamContent()
        streamMachine = null; stopSpinner()
      }
      // Start a fresh session — clear screen, re-render banner, reset state
      sessionId = null
      appState = { ...createInitialState(appState.model, agent.cwd), verbose: appState.verbose }
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      try { preloadedSessions = await agent.listSessions(20) } catch {}
      const banner = currentBannerText()
      renderer.appendScroll(banner)
      const match = findPreviousSession(preloadedSessions, agent.cwd)
      if (match) commitLines([previousSessionLine(match)])
    }
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

    if (name === '/goto') {
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
        // Fetch a large set for search, display only the most recent entries
        const displayLimit = args ? parseInt(args, 10) : 20
        const searchLimit = Math.max(displayLimit, 200)
        const searchCmd = `/history ${searchLimit}`
        const outcome = await agent.submit(searchCmd, sessionId ?? undefined)
        if (outcome.kind === 'command') {
          const allItems = parseHistoryItems(outcome.message)
          if (allItems.length === 0) {
            commitLines([{ id: 'sys-hist', kind: 'system', text: `  ${outcome.message}` }])
          } else {
            // Mark user entries as goto-able, assistant as preview-only
            const annotate = (items: typeof allItems) => items.map(item => ({
              ...item,
              detail: item.role === 'user' ? `↩ ${item.detail}` : `  ${item.detail}`,
              focusable: item.role === 'user',
            }))
            const displayItems = allItems.slice(-displayLimit)
            overlay = {
              kind: 'selector',
              state: createSelectorState('History  (↩ goto · enter preview)', annotate(displayItems), annotate(allItems)),
            }
          }
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
      try {
        if (args && isSessionIdPrefix(args)) {
          const allSessions: SessionMeta[] = await agent.listSessions(0)
          const resolved = resolveSessionByPrefix(allSessions, args)
          if (resolved.kind === 'matched') {
            await resumeSession(resolved.session)
          } else {
            openResumeSelector(args)
          }
        } else {
          openResumeSelector(args || undefined)
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
          state: {
            ...createSelectorState('Select model', models.map(m => ({
              label: m,
              detail: m === agent.model ? '(current)' : undefined,
            }))),
            focusIndex: Math.max(0, models.indexOf(agent.model)),
          },
        }
      } else {
        commitLines([{ id: 'sys-m', kind: 'system', text: '  Only one model available.' }])
      }
    }

    renderStatus()
  }

  function openResumeSelector(initialQuery?: string) {
    agent.listSessions(0).then(allSessions => {
      const pool = selectSessionPool(allSessions, agent.cwd)
      if (pool.length === 0) {
        commitLines([{ id: 'sys-r', kind: 'system', text: '  No sessions found' }])
        return
      }
      const metaItems = formatSessionItems(pool.slice(0, 20))
      const allMetaItems = formatSessionItems(pool)
      overlay = {
        kind: 'selector',
        state: createSelectorState(RESUME_SELECTOR_TITLE, metaItems, allMetaItems, initialQuery),
      }
      renderStatus()
      agent.listSessionsWithText(0).then(allWithText => {
        if (overlay.kind !== 'selector' || !isResumeSelectorTitle(overlay.state.title)) return
        const fullPool = selectSessionPool(allWithText, agent.cwd)
        const fullItems = formatSessionWithTextItems(fullPool)
        overlay = {
          kind: 'selector',
          state: selectorExpandItems(overlay.state, fullItems),
        }
        renderStatus()
      }).catch(() => {})
    }).catch((err: any) => {
      commitLines([{ id: 'sys-r-err', kind: 'system', text: chalk.red(`  Failed to list sessions: ${err?.message ?? err}`) }])
    })
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
        commitLines([{ id: 'sys-log-mode', kind: 'system', text: `  [log mode] analyzing: ${logPath}\n  not persisted. press Esc to exit.` }])
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
        } else if (event.kind === 'tool_progress') {
          if (streamingText.trim()) { commitLines(buildAssistantLines(streamingText)); streamingText = '' }
          commitLines(buildToolProgressLines(event, true))
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
    const action = handleSelectorControl(overlay.state, event)

    switch (action.kind) {
      case 'update':
        overlay = { kind: 'selector', state: action.state }
        renderStatus()
        return
      case 'close':
        overlay = { kind: 'none' }
        renderStatus()
        return
      case 'resume':
        overlay = { kind: 'none' }
        resumeSession({ session_id: action.sessionId } as SessionMeta).then(() => renderStatus())
        renderStatus()
        return
      case 'history-goto':
        overlay = { kind: 'none' }
        handleSlashInput(`/goto ${action.seq}`)
        renderStatus()
        return
      case 'history-preview':
        overlay = { kind: 'none' }
        commitLines([{ id: 'sys-hist-preview', kind: 'system', text: `  ${action.label} assistant: ${action.text}` }])
        renderStatus()
        return
      case 'select-model':
        overlay = { kind: 'none' }
        agent.model = action.model
        syncProvider(agent, action.model, configInfo)
        appState = { ...appState, model: action.model }
        commitLines([{ id: 'sys-model', kind: 'system', text: `  Model → ${action.model}` }])
        renderStatus()
        return
      case 'delete-session':
        overlay = { kind: 'selector', state: action.state }
        agent.deleteSession(action.sessionId).then(ok => {
          if (ok) {
            commitLines([{ id: 'sys-del', kind: 'system', text: `  Deleted session ${action.label}` }])
          }
        })
        renderStatus()
        return
      case 'none':
        return
    }
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
        flushStreamContent()
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
