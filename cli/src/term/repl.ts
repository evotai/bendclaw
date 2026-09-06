import { TermRenderer } from './renderer.js'
import type { RenderFrame } from './render-frame.js'
import { watchOutputFile } from './app/output-watch.js'
import { readOutputTail } from './app/output-tail.js'
import {
  enableRawMode,
  enableEnhancedKeyboard,
  type EnhancedKeyboardSession,
  type KeyEvent,
  type TerminalControlEvent,
} from './input.js'
import { TerminalInputBuffer } from './input/buffer.js'
import { schemeFromRgbColor } from './terminal-colors.js'
import { getTheme, setDetectedThemeScheme } from '../render/theme/index.js'
import { createSpinnerState, advanceSpinner, formatSpinnerLine, setSpinnerPhase, spinnerStatsFromLastUsage } from './spinner.js'
import { createModelWindow, createResumeWindow } from './app/selector-windows.js'
import { buildShellFrame } from './viewmodel/shell.js'
import { promptFromSnapshot } from './viewmodel/prompt-snapshot.js'
import { createAppSelectorState, isCommandSelector, isBackgroundSelector, SELECTOR_OWNER } from './app/selector-identity.js'
import { selectorExpandItems, selectorClearQuery, selectorReplaceItem, warmSearchableText, type SelectorItem, type SelectorState } from './selector.js'
import { createAskState, handleAskKeyEvent, type AskQuestion } from './ask.js'
import { buildAssistantLines, buildUserMessage, messagesToOutputLines, type OutputLine } from '../render/output.js'
import { manualCompactionLines } from './viewmodel/manual-compaction.js'
import { wrapTextWithAnsi } from '../render/wrap.js'
import { Agent, QueryStream, fastExit, authNotices, type ManualCompactionOutcome, type SessionMeta, type ConfigInfo } from '../native/index.js'
import { createInitialState, type AppState } from './app/state.js'
import { assistantToolCalls } from './app/assistant-content.js'
import type { UIAssistantBlock } from './app/types.js'
import { assistantMessageToOutputLines } from '../render/assistant.js'
import { HistoryManager } from '../session/history.js'
import { ScreenLog } from '../session/screen-log.js'
import { SessionHook } from '../session/hook.js'
import { RendererTrace } from '../session/renderer-trace.js'
import { findLastAssistantMarkdown, findLastAssistantTurn } from '../session/assistant-markdown.js'
import { isSlashCommand, resolveCommand, buildHardenPrompt } from '../commands/index.js'
import { BannerCache } from './banner-cache.js'
import {
  buildOutputBlocks,
  updateLiveHeight,
  formatQueuedMessageLines,
  blocksToLines,
  type PromptVMInput,
  type ViewBlock,
} from './viewmodel/index.js'
import { updateNoticeSpans, attachUpdateNotice, standaloneUpdateNotice } from './viewmodel/update-notice.js'
import { createAdSlotState, tickAdSlot, nextAdSlotRenderDelay, triggerAdSlot, queueAdSlotTransition, buildAdSlotBlocks, campaignFingerprint, type AdSlotState } from './viewmodel/ad-slot.js'
import { HistoryRenderCache } from './viewmodel/history-cache.js'
import { Committer } from './committer.js'
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
  acceptCompletion,
  closeCompletion,
  moveCompletion,
  refreshGhostHint,
  createHistoryState,
  pushHistory,
  historyPrev,
  historyNext,
  clearLineBefore,
  clearLineAfter,
  deleteForward,
  deleteWordBefore,
  deleteWordForward,
  insertNewline,
  insertContinuationNewline,
  moveUp,
  moveDown,
  moveWordLeft,
  moveWordRight,
  editorNeedsContinuation,
  type EditorState,
  type HistoryState,
} from './input/editor.js'
import { UndoStack } from './input/undo-stack.js'
import {
  createStreamMachineState,
  reduceRunEvent,
  flushStreaming,
  type StreamMachineState,
} from './app/stream.js'
import { handleSlashCommand } from './app/commands.js'
import { askStateToResponse } from './app/ask-user.js'
import { RunOwnership } from './app/run-ownership.js'
import {
  dispatchHostToolCall,
  HOST_TOOL_SPECS_JSON,
  type AskUserAnswer,
  type AskUserParams,
} from './host-tools.js'
import { extractPlanItems, type PlanModeItem } from './plan-mode.js'
import { currentModelSpec, formatModelLabel, formatModelOptionLabel, hasPremiumModel, isCloudModel, modelOptions, modelSelectorItems, selectModelOption } from './app/provider.js'
import chalk from 'chalk'
import {
  shouldCollapse,
  cleanPastedText,
  formatPastedTextRef,
  formatImageRef,
  parsePasteRefs,
  resolveHistoryText,
  deleteRefBackspace,
  resolveSubmitText,
} from './input/paste_refs.js'
import { getImageFromClipboard } from './input/clipboard_image.js'
import { getTextFromClipboard } from './input/clipboard_text.js'
import { InputImageHistory } from './input/image-history.js'
import { storeImage, formatImageSourceText } from './input/image_store.js'
import type { ContentBlock } from '../native/index.js'
import { tryStartServer, type ServerState } from './app/server.js'
import {
  RESUME_SELECTOR_TITLE,
  COMPACT_SUMMARY_PREFIX,
  applySessionText,
  formatSessionItems,
  isSessionIdPrefix,
  normalizeResumeQuery,
  resolveSessionByPrefix,
  shortenSessionCwd,
} from './app/resume.js'
import { findPreviousSession, shouldPreloadStartupSessions, selectResumeMessages, resumeElidedLine, reloadResumeModel } from './app/session-view.js'
import { handleSelectorControl } from './app/selector-control.js'
import { decideReplControl, type ReplControlAction } from './app/repl-control.js'
import { replaceOrPushStatusLine } from './app/status-line.js'
import { AuthWatcher } from './app/auth-watch.js'
import { InterruptConfirmation } from './app/interrupt-confirmation.js'
import { ManualCompaction } from './app/manual-compaction.js'
import { busySubmissionAction } from './app/busy-submission.js'
import { mergeQueuedIntoEditorText } from './app/queue-restore.js'
import { BackgroundTerminals } from './app/background-terminals.js'
import { isBackgroundPanelShortcut } from './app/background-panel.js'
import {
  createQueueSelectorState,
  isQueueManageShortcut,
  type ManagedQueuedPrompt,
} from './app/queue-manage.js'
import { FileCompletion } from './app/file-completion.js'
import { extractAtPrefix, completeAtFile } from '../commands/file-completion.js'
import { getSkillEntries } from '../commands/skill.js'
import { isCommandWindowBridge, isCommandWindowTypingEvent, resolveCommandWindowTrigger } from './app/command-window-trigger.js'
import { createSkillSelectorState } from './app/skill-window.js'
import { transcriptToMessages } from '../session/transcript.js'
import { GitInfoProvider } from './git-info.js'
import { isHostToolEvent } from '../native/contracts/query-event.js'
import { ResumeSessionCache } from './app/resume-cache.js'
import { CloudSync } from './app/cloud-sync.js'
import { prepareResume } from './app/prepare-resume.js'
import { campaignContent, refreshCampaigns } from './app/campaigns.js'
import type { OverlayState } from './app/overlay-state.js'
import { ResourceScope } from './resource-scope.js'
import { RenderWakeup } from './render-wakeup.js'
import { CaretBlink } from './caret-blink.js'
import { errorText } from '../render/format.js'
import { TerminalTitle } from './title.js'
import {
  formatLogPaths,
  handleClipCommand,
  handleCopyCommand,
  handleEnvCommand,
  handleShareCommand,
  handleLoginCommand,
  handleLogoutCommand,
  handleSkillCommand,
  handleUpdateCommand,
  handleVersionCommand,
  type ReplCommandContext,
} from './repl-commands.js'

const SPINNER_INTERVAL_MS = 100

import { readPromptQueues, visibleQueueEntries, reconcilePromptQueue, type QueuedUserMessage } from './app/prompt-queue.js'
type QueuedCompactionSubmission = { displayText: string; expandedText: string; contentJson?: string }
type CommandWindowPreview =
  | { kind: 'selector'; trigger: 'model' | 'resume' | 'skill'; sourceText: string; generation: number; state: SelectorState }
  | { kind: 'help'; trigger: 'help'; sourceText: string; generation: number }


export interface ReplOptions {
  agent: Agent
  resumeSessionId?: string
  continueLatest?: boolean
  serverPort?: number
  envFile?: string
}

export async function startRepl(opts: ReplOptions): Promise<void> {
  const { agent } = opts
  const { version } = await import('../native/index.js')
  const appVersion = version()
  const rendererTrace = new RendererTrace()
  const sessionHook = new SessionHook({ cwd: agent.cwd })
  sessionHook.startProcess(agent.cwd)
  const renderer = new TermRenderer({
    trace: rendererTrace.isEnabled ? entry => rendererTrace.log(entry) : undefined,
  })
  renderer.init()

  // The composer paints its own caret, so the idle blink is ours to drive.
  const resources = new ResourceScope()
  const backgroundCleanup = new ResourceScope()
  const caretBlink = new CaretBlink({ onChange: () => renderer.requestRender() })
  // Armed by buildFrame only while visible text or its lifecycle needs a wakeup.
  const adSlotWakeup = new RenderWakeup(() => renderer.requestRender())

  let appState: AppState = {
    ...createInitialState(agent.model, agent.cwd),
  }
  let spinnerState = createSpinnerState()
  /**
   * Spinner state for the idle wait on a detached task.
   *
   * Separate from `spinnerState` so the two never overwrite each other: a run
   * starting mid-wait resets its own animation, and the wait keeps its own
   * elapsed clock rather than inheriting the last run's.
   */
  let backgroundWaitSpinner = setSpinnerPhase(createSpinnerState(), 'awaiting_background')
  let backgroundWaitSince: number | null = null
  let manualCompactionPhase: string | null = null
  let editor: EditorState = createEditorState()
  // Undo stack lives outside EditorState so snapshots stay plain data. Cleared
  // on submit/clear so a previous prompt cannot be restored into a new one.
  const editorUndo = new UndoStack<EditorState>()
  let historyState: HistoryState
  let isLoading = false
  let loginInFlight = false
  // Local foreground commands reuse the animated status row without pretending
  // they are abortable agent streams or showing stale LLM usage.
  let foregroundCommand: 'log-shot' | null = null
  let streamRef: QueryStream | null = null
  // Native abort settles asynchronously. Ownership is revoked before aborting
  // so an old Promise cannot report twice or clear a newer run's state.
  const interruptConfirmation = new InterruptConfirmation()
  /** The operation an interrupt would stop; a new run never inherits an armed window. */
  const interruptOwner = (): unknown => manualCompaction.active ? manualCompaction : streamRef
  const runOwnership = new RunOwnership()
  const beginRun = (): number => runOwnership.begin()
  const ownsRun = (generation: number): boolean => runOwnership.owns(generation)
  const revokeRun = (): void => runOwnership.revoke()
  const manualCompaction = new ManualCompaction()
  let queuedCompactionSubmissions: QueuedCompactionSubmission[] = []
  let spinnerTimer: ReturnType<typeof setInterval> | null = null
  const terminalTitle = new TerminalTitle(agent.cwd, () => serverState?.port ?? null)
  const setTerminalTitle = terminalTitle.set.bind(terminalTitle)
  const freezeTerminalTitle = terminalTitle.freeze.bind(terminalTitle)
  const unfreezeTerminalTitle = terminalTitle.unfreeze.bind(terminalTitle)
  const replCommands: ReplCommandContext = {
    agent,
    getSessionId: () => sessionId,
    getCompactLines: () => compactLines,
    getConfigInfo: () => configInfo ?? null,
    commitSystem,
    commitRevealed,
    commitLines,
    replaceLine: (id, text) => committer.replaceById(id, text),
    columns: () => renderer.termCols,
    requestRender: () => renderer.requestRender(),
  }
  let destroyed = false
  let disableRaw: (() => void) | null = null
  let enhancedKeyboard: EnhancedKeyboardSession | null = null
  let inputBuffer: TerminalInputBuffer | null = null
  let escapeFlushTimer: ReturnType<typeof setTimeout> | undefined
  let onInputData: ((data: Buffer | string) => void) | null = null
  let sessionId: string | null = null
  let nextBackgroundLineId = 1
  let planning = false
  let logMode: import('../native/index.js').ForkedAgent | null = null
  let exitHint = false
  let exitHintTimer: ReturnType<typeof setTimeout> | null = null
  let overlay: OverlayState = { kind: 'none' }
  let commandWindowPreview: CommandWindowPreview | null = null
  let commandWindowPreviewGeneration = 0
  let focusedCommandWindowGeneration: number | null = null
  let commandWindowContentLines: string[] | null = null
  let commandWindowContentWidth: number | null = null
  let deferredCloudModelNotice: string | null = null
  let deferredCloudCampaignId: string | null = null
  let enrichedResumeMetadataGeneration: number | null = null
  let enrichedResumeTextGeneration: number | null = null
  let resumeCommandLoadTimer: ReturnType<typeof setTimeout> | undefined
  let resumeSearchEnrichmentTimer: ReturnType<typeof setTimeout> | undefined
  /** Cancels the in-progress search-text warm-up, if one is running. */
  let cancelSearchTextWarmup: (() => void) | null = null
  let preloadedSessions: SessionMeta[] = []
  const resumeCache = new ResumeSessionCache(agent, rows => { preloadedSessions = rows })
  resources.add(() => resumeCache.dispose())
  /** Invalidates callbacks belonging to an explicitly submitted `/resume`. */
  let explicitResumeSelectorGeneration = 0
  // Assigned after configInfo below, which decides the premium intake filter.
  let adSlot: AdSlotState
  // Promise-based bridge for the ask-user overlay. Both the `ask_user` host
  // tool and the plan-review flow present questions through the same overlay
  // and await the user's answers here. Resolves with the collected answers, or
  // null when the user cancels/skips.
  let pendingAsk: ((answers: AskUserAnswer[] | null) => void) | null = null

  /** Resolve any awaiting ask/plan-review overlay as cancelled. Safe to call
   *  on every teardown path (interrupt, cancel, overlay close, re-present) so a
   *  suspended host-tool dispatch never strands the run loop. */
  function resolvePendingAsk() {
    if (pendingAsk) {
      pendingAsk(null)
      pendingAsk = null
    }
  }

  function presentAskQuestions(
    questions: AskQuestion[],
    resumeState: 'working' | 'idle' = 'working',
  ): Promise<AskUserAnswer[] | null> {
    // Only one ask overlay can be active at a time; resolve any prior one as
    // cancelled before opening the next.
    resolvePendingAsk()
    sessionHook.state('blocked')
    overlay = { kind: 'ask-user', state: createAskState(questions) }
    freezeTerminalTitle('?')
    renderer.requestRender()
    return new Promise(resolve => {
      pendingAsk = answers => {
        sessionHook.state(resumeState)
        resolve(answers)
      }
    })
  }

  const collectAskUserAnswers = (params: AskUserParams) =>
    presentAskQuestions(
      params.questions.map(q => ({
        header: q.header,
        question: q.question,
        options: q.options.map(o => ({ label: o.label, description: o.description })),
      })),
    )

  // Plan-mode state (pi-style): `/plan` enters read-only planning, the model
  // writes a `Plan:` section, and after the turn the extracted steps drive an
  // Execute / Stay / Refine review. Progress is not rendered as a sticky
  // checklist (it only advances on turn-end [DONE:n] tags and is easy to stick
  // when a step is never tagged). The review overlay owns the plan display.
  let planModeItems: PlanModeItem[] = []
  let lastReviewedPlanMarkdown = ''

  function latestAssistantMarkdown(): string | null {
    return findLastAssistantMarkdown(compactLines)?.rawMarkdown ?? null
  }

  async function maybeReviewPlanAfterTurn(): Promise<void> {
    if (!planning) return
    const markdown = latestAssistantMarkdown()
    if (!markdown || markdown === lastReviewedPlanMarkdown) return
    const extracted = extractPlanItems(markdown)
    if (extracted.length === 0) return

    lastReviewedPlanMarkdown = markdown
    planModeItems = extracted
    renderer.requestRender()

    const planList = planModeItems.map(item => `${item.step}. ☐ ${item.text}`).join('\n')
    const answers = await presentAskQuestions([
      {
        header: 'Plan',
        question: `Plan mode - what next?\n${planList}\n\nChoose an action, or type refinement feedback as custom text.`,
        options: [
          { label: 'Execute the plan', description: 'Leave plan mode and restore write tools.' },
          { label: 'Stay in plan mode', description: 'Keep planning without executing yet.' },
          { label: 'Refine the plan', description: 'Return to the prompt to enter refinement feedback.' },
        ],
      },
    ], 'idle')
    if (!answers || answers.length === 0) return

    const choice = answers[0]!.answer
    if (choice === 'Execute the plan') {
      planning = false
      commitSystem('sys-plan-exec', '  planning: off · executing plan')
      const remaining = planModeItems
        .map(item => `${item.step}. ${item.text}`)
        .join('\n')
      // Drop the sticky checklist for execution: it only advanced on turn-end
      // [DONE:n] tags and stuck when a step was never tagged.
      planModeItems = []
      const execMessage = `Execute the plan.\n\nRemaining steps:\n${remaining}\n\nExecute each step in order.`
      commitLines(buildUserMessage(execMessage))
      await runQuery(execMessage)
      return
    }

    if (choice === 'Stay in plan mode' || choice === 'Skipped') {
      commitSystem('sys-plan-stay', '  planning: on · staying in plan mode')
      renderer.requestRender()
      return
    }

    if (choice === 'Refine the plan') {
      editor = insertText(clearEditor(editor), 'Refine the plan: ')
      commitSystem('sys-plan-refine', '  planning: on · enter refinement feedback')
      renderer.requestRender()
      return
    }

    commitLines(buildUserMessage(choice))
    await runQuery(choice)
  }
  let streamMachine: StreamMachineState | null = null
  // Messages sent mid-stream: held in the prompt zone (pi-style ❯ queue) and
  // committed to history when steering consumes them at the next safe boundary,
  // so they never render above the still-streaming reply.
  let queuedUserMessages: QueuedUserMessage[] = []
  let editingQueuedPrompt: ManagedQueuedPrompt | null = null
  let stashedQueueEditDraft = ''
  let expanded = false
  // Rendered-history cache — see HistoryRenderCache. Committed history is
  // append-only (or fully cleared), never mutated in place, so the flattened
  // ANSI lines are extended incrementally instead of re-flattened every frame.
  // A full rebuild over a long session takes 5–14 ms, which is what made the
  // just-sent message and keystroke echo visibly stall as the conversation
  // grew. Compact and expanded views each get their own cache because their
  // source arrays are appended independently (expanded-only progress/thinking).
  const compactHistoryCache = new HistoryRenderCache()
  const expandedHistoryCache = new HistoryRenderCache()
  function resetHistoryCache() {
    compactHistoryCache.reset()
    expandedHistoryCache.reset()
  }

  function nextCommandWindowGeneration(): number {
    commandWindowPreviewGeneration++
    return commandWindowPreviewGeneration
  }

  function commandWindowMounted(): boolean {
    return commandWindowPreview !== null || focusedCommandWindowGeneration !== null
  }

  function showCloudCampaign(campaignId: string): void {
    if (!queueAdSlotTransition(adSlot, campaignId)) {
      triggerAdSlot(adSlot, Date.now())
    }
  }

  function flushDeferredCloudUpdates(): void {
    if (commandWindowMounted()) return
    const modelNotice = deferredCloudModelNotice
    const campaignId = deferredCloudCampaignId
    deferredCloudModelNotice = null
    deferredCloudCampaignId = null
    if (modelNotice) commitSystem('sys-cloud-models', chalk.dim(modelNotice))
    if (campaignId) showCloudCampaign(campaignId)
  }

  function releaseCommandWindowLayout(): void {
    commandWindowContentLines = null
    commandWindowContentWidth = null
    // The window is gone, so its search-text warm-up has nothing left to serve.
    // Cancelling here rather than per keystroke keeps warming alive across
    // typing inside an open window, which is exactly when it is needed.
    cancelSearchTextWarm()
    flushDeferredCloudUpdates()
  }

  function modelSelectorState(): SelectorState {
    return createModelWindow(configInfo, agent.model)
  }

  function skillSelectorState(): SelectorState {
    return { ...createSkillSelectorState(getSkillEntries(agent.skillsDirs())), listFocused: false }
  }

  function resumeSelectorState(items: SelectorItem[], initialQuery?: string): SelectorState {
    return createResumeWindow(items, initialQuery)
  }

  function currentResumeCommandWindowState(generation: number): SelectorState | null {
    if (generation !== commandWindowPreviewGeneration) return null
    if (
      commandWindowPreview?.kind === 'selector'
      && commandWindowPreview.trigger === 'resume'
      && commandWindowPreview.generation === generation
      && resolveCommandWindowTrigger(commandWindowPreview.sourceText) === 'resume'
    ) {
      return commandWindowPreview.state
    }
    if (
      focusedCommandWindowGeneration === generation
      && overlay.kind === 'selector'
      && overlay.state.owner === SELECTOR_OWNER.resume
    ) {
      return overlay.state
    }
    return null
  }

  function updateResumeCommandWindow(generation: number, state: SelectorState): boolean {
    if (generation !== commandWindowPreviewGeneration) return false
    if (
      commandWindowPreview?.kind === 'selector'
      && commandWindowPreview.trigger === 'resume'
      && commandWindowPreview.generation === generation
    ) {
      commandWindowPreview = { ...commandWindowPreview, state }
      renderer.requestRender()
      return true
    }
    if (
      focusedCommandWindowGeneration === generation
      && overlay.kind === 'selector'
      && overlay.state.owner === SELECTOR_OWNER.resume
    ) {
      overlay = { kind: 'selector', state }
      renderer.requestRender()
      return true
    }
    return false
  }

  function resumeSelectorStateFromCache(): SelectorState {
    // Keep every keystroke bounded: the composer preview is recognition aid,
    // not the full search surface. Never format transcript text here, and cap
    // metadata work to the same recent-session budget used at startup.
    const recent = (resumeCache.metadata ?? []).slice(0, 20)
    if (recent.length > 0) {
      const items = formatSessionItems(recent, agent.cwd, id => resumeCache.sessionText(id))
      return resumeSelectorState(items)
    }
    if (resumeCache.complete) {
      return {
        ...createAppSelectorState('resume', RESUME_SELECTOR_TITLE, []),
        emptyMessage: 'No sessions found',
      }
    }
    return {
      ...createAppSelectorState('resume', RESUME_SELECTOR_TITLE, []),
      emptyMessage: 'Loading sessions…',
    }
  }

  function applyResumeSessions(
    generation: number,
    sessions: SessionMeta[],
    limit?: number,
  ): boolean {
    const current = currentResumeCommandWindowState(generation)
    if (!current) return false
    const visibleSessions = limit === undefined ? sessions : sessions.slice(0, limit)
    const items = formatSessionItems(visibleSessions, agent.cwd, id => resumeCache.sessionText(id))
    const {
      emptyMessage: _loadingMessage,
      subtitle: _loadingSubtitle,
      ...readyState
    } = current
    const expanded = selectorExpandItems(readyState, items)
    const hasAnySession = items.some(item => !item.header)
    const next = expanded.items.length > 0
      ? expanded
      : {
          ...expanded,
          emptyMessage: hasAnySession
            ? 'No sessions in current cwd · type to search all sessions'
            : 'No sessions found',
        }
    const applied = updateResumeCommandWindow(generation, next)
    if (applied) loadFocusedResumePreview(next)
    return applied
  }

  function cancelResumeCommandLoad(): void {
    if (!resumeCommandLoadTimer) return
    clearTimeout(resumeCommandLoadTimer)
    resumeCommandLoadTimer = undefined
  }

  function isActiveResumePreview(generation: number): boolean {
    return generation === commandWindowPreviewGeneration
      && commandWindowPreview?.kind === 'selector'
      && commandWindowPreview.trigger === 'resume'
      && commandWindowPreview.generation === generation
      && resolveCommandWindowTrigger(commandWindowPreview.sourceText) === 'resume'
  }

  function scheduleResumeCommandLoad(generation: number): void {
    cancelResumeCommandLoad()
    if (!isActiveResumePreview(generation)) return

    const current = currentResumeCommandWindowState(generation)
    if (current && resumeCache.metadata === null) {
      updateResumeCommandWindow(
        generation,
        current.items.length === 0
          ? { ...current, emptyMessage: 'Loading sessions…' }
          : { ...current, subtitle: 'Loading sessions…' },
      )
    }

    // The loading frame is requested above. Start native work only after a
    // short keyboard-idle window. Every edit rekeys the mounted preview, so an
    // older native result cannot repaint a newer `/re` → `/` transition.
    //
    // Paint the bounded cache first, then expand metadata from the complete
    // catalog in place. The startup cache is global-recency based: after cwd
    // filtering it may contain only a couple of rows even though this project
    // has much more history. Stopping at that cache made the live command
    // window disagree with the selector opened by Enter.
    resumeCommandLoadTimer = setTimeout(() => {
      resumeCommandLoadTimer = undefined
      if (!isActiveResumePreview(generation)) return
      void resumeCache.preview().then(sessions => {
        if (!isActiveResumePreview(generation)) return
        applyResumeSessions(generation, sessions, 20)

        void resumeCache.all().then(allSessions => {
          if (!isActiveResumePreview(generation)) return
          if (applyResumeSessions(generation, allSessions)) {
            enrichedResumeMetadataGeneration = generation
          }
        }).catch(() => {
          // The bounded preview is already usable. A failed enrichment should
          // not replace real rows with an error; only clear its loading label.
          if (!isActiveResumePreview(generation)) return
          const state = currentResumeCommandWindowState(generation)
          if (!state) return
          updateResumeCommandWindow(generation, { ...state, subtitle: undefined })
        })
      }).catch(() => {
        if (!isActiveResumePreview(generation)) return
        const state = currentResumeCommandWindowState(generation)
        if (!state) return
        updateResumeCommandWindow(generation, {
          ...state,
          emptyMessage: 'Failed to list sessions',
          subtitle: undefined,
        })
      })
    }, 160)
  }

  function cancelSearchTextWarm(): void {
    if (!cancelSearchTextWarmup) return
    cancelSearchTextWarmup()
    cancelSearchTextWarmup = null
  }

  /**
   * Convert transcript search text to lowercase ahead of the first keystroke.
   *
   * Filtering needs a lowercased copy of every row. Building it on demand put
   * the whole 14M-character conversion on whichever keystroke came first, which
   * read as a stall right after the list finished loading. Warming in idle
   * slices moves that off the typing path entirely.
   */
  function startSearchTextWarm(items: SelectorItem[]): void {
    cancelSearchTextWarm()
    cancelSearchTextWarmup = warmSearchableText(items)
  }

  function cancelResumeSearchEnrichment(): void {
    if (!resumeSearchEnrichmentTimer) return
    clearTimeout(resumeSearchEnrichmentTimer)
    resumeSearchEnrichmentTimer = undefined
  }

  function scheduleFocusedResumeEnrichment(generation: number, includeText = false): void {
    cancelResumeCommandLoad()
    cancelResumeSearchEnrichment()
    if (
      includeText
        ? enrichedResumeTextGeneration === generation
        : enrichedResumeMetadataGeneration === generation
    ) return

    // Keep keyboard response ahead of storage work. The bounded 20-row preview
    // is already usable; only after a short input idle do we expand metadata.
    // Complete caches still pass through this idle boundary: reopening `/re`
    // starts compact, then expands from memory without native I/O. Transcript
    // parsing is deferred until a typed filter can benefit from full text.
    resumeSearchEnrichmentTimer = setTimeout(() => {
      resumeSearchEnrichmentTimer = undefined
      if (!currentResumeCommandWindowState(generation)) return

      const metadata = resumeCache.all()
      void metadata.then(sessions => {
        if (!applyResumeSessions(generation, sessions)) return
        enrichedResumeMetadataGeneration = generation
        if (!includeText) return

        return resumeCache.text().then(sessionsWithText => {
          const current = currentResumeCommandWindowState(generation)
          if (!current) return
          const fullItems = formatSessionItems(sessionsWithText, agent.cwd, id => resumeCache.sessionText(id))
          if (updateResumeCommandWindow(generation, selectorExpandItems(current, fullItems))) {
            enrichedResumeTextGeneration = generation
            // Transcript text just became searchable. Lowercase it in idle
            // slices so the first keystroke does not pay for the whole pool.
            startSearchTextWarm(fullItems)
          }
        })
      }).catch(() => {
        const current = currentResumeCommandWindowState(generation)
        if (!current) return
        updateResumeCommandWindow(generation, {
          ...current,
          emptyMessage: 'Failed to list sessions',
        })
      })
    }, 160)
  }

  /**
   * The resume list currently on screen, with the setter that owns it.
   *
   * A resume list lives either in the composer preview or in the promoted
   * overlay. Both need the focused row's preview filled in, so the loader
   * addresses whichever one is mounted instead of only the overlay.
   */
  function resumeSurface(): { state: SelectorState; apply: (state: SelectorState) => void } | null {
    if (commandWindowPreview?.kind === 'selector' && commandWindowPreview.trigger === 'resume') {
      const mounted = commandWindowPreview
      return { state: mounted.state, apply: state => { commandWindowPreview = { ...mounted, state } } }
    }
    if (overlay.kind === 'selector' && overlay.state.owner === SELECTOR_OWNER.resume) {
      return { state: overlay.state, apply: state => { overlay = { kind: 'selector', state } } }
    }
    return null
  }

  /**
   * Fill the focused row's preview from its own transcript.
   *
   * The pane shows one session at a time, so only that session is read. Reading
   * the whole catalog to render one pane costs a transcript read per session,
   * which on a large history left the pane empty long enough to look broken.
   */
  function loadFocusedResumePreview(state: SelectorState): void {
    const focused = state.items[state.focusIndex]
    if (!focused || focused.header || !focused.id) return
    const id = focused.id
    void resumeCache.loadSessionText(id).then(text => {
      if (!text) return
      const surface = resumeSurface()
      const stillFocused = surface?.state.items[surface.state.focusIndex]
      // Focus moved on, or the list was replaced: a later focus reloads from
      // the cache, so there is nothing to reconcile here.
      if (!surface || !stillFocused || stillFocused.id !== id) return
      surface.apply(selectorReplaceItem(surface.state, id, applySessionText(stillFocused, text, agent.cwd)))
      renderer.requestRender()
    })
  }

  function refreshCommandWindowPreview(allowMount: boolean): void {
    if (overlay.kind !== 'none' || isLoading || editingQueuedPrompt) {
      // A promoted command window continues to own its generation so an
      // in-flight resume load can update the focused selector. Other overlays
      // invalidate any stale preview request.
      if (commandWindowPreview) {
        commandWindowPreview = null
        cancelResumeCommandLoad()
        nextCommandWindowGeneration()
        releaseCommandWindowLayout()
      } else if (focusedCommandWindowGeneration === null) {
        nextCommandWindowGeneration()
      }
      return
    }

    // Nothing is mounted, so the only possible outcome is mounting a new
    // window. Reserve that for typing: ↑/↓ recalling `/model` from history or
    // pasting it must not pop the window above the composer.
    if (commandWindowPreview === null && !allowMount) return

    const sourceText = getEditorText(editor)
    const trigger = resolveCommandWindowTrigger(sourceText)
    if (!trigger) {
      if (commandWindowPreview && isCommandWindowBridge(sourceText)) {
        // Keep the mounted window through an ambiguous slash prefix. Rekey it
        // so any session request started for the previous spelling may fill
        // caches but can no longer repaint this bridge frame.
        const preview = commandWindowPreview
        const generation = nextCommandWindowGeneration()
        commandWindowPreview = { ...preview, sourceText, generation }
        cancelResumeCommandLoad()
        renderer.requestRender()
        return
      }
      if (commandWindowPreview) {
        commandWindowPreview = null
        cancelResumeCommandLoad()
        nextCommandWindowGeneration()
        releaseCommandWindowLayout()
        renderer.requestRender()
      }
      return
    }
    if (commandWindowPreview?.trigger === trigger) {
      const preview = commandWindowPreview
      if (preview.sourceText === sourceText) return
      const generation = nextCommandWindowGeneration()
      commandWindowPreview = { ...preview, sourceText, generation }
      renderer.requestRender()
      if (trigger === 'resume') scheduleResumeCommandLoad(generation)
      return
    }

    cancelResumeCommandLoad()
    const generation = nextCommandWindowGeneration()
    if (trigger === 'help') {
      commandWindowPreview = { kind: 'help', trigger, sourceText, generation }
      renderer.requestRender()
      return
    }
    if (trigger === 'model') {
      commandWindowPreview = {
        kind: 'selector',
        trigger,
        sourceText,
        generation,
        state: modelSelectorState(),
      }
      renderer.requestRender()
      return
    }
    if (trigger === 'skill') {
      commandWindowPreview = {
        kind: 'selector',
        trigger,
        sourceText,
        generation,
        state: skillSelectorState(),
      }
      renderer.requestRender()
      return
    }

    commandWindowPreview = {
      kind: 'selector',
      trigger,
      sourceText,
      generation,
      state: resumeSelectorStateFromCache(),
    }
    renderer.requestRender()
    scheduleResumeCommandLoad(generation)
  }

  function activateCommandWindow(event: KeyEvent): boolean {
    if (event.type !== 'up' && event.type !== 'down') return false
    if (!commandWindowPreview) return false

    const preview = commandWindowPreview
    focusedCommandWindowGeneration = preview.generation
    if (preview.kind === 'selector') {
      // Use the same navigation path as an open selector so the first arrow
      // both focuses the list and moves from the preview's highlighted row.
      const action = handleSelectorControl(preview.state, event)
      if (action.kind !== 'update') return false
      overlay = { kind: 'selector', state: action.state }
    } else {
      overlay = { kind: 'help' }
    }
    commandWindowPreview = null
    // Help remains a modal rather than participating in the stable
    // selector/composer layout. Consume its command on activation so closing
    // the modal cannot immediately recreate the same preview from `/help`.
    if (preview.kind === 'help') clearAll()
    renderer.requestRender()
    if (preview.trigger === 'resume') {
      scheduleFocusedResumeEnrichment(preview.generation)
      if (overlay.kind === 'selector') loadFocusedResumePreview(overlay.state)
    }
    return true
  }

  function mutateEditor(mutator: (state: EditorState) => EditorState): void {
    const previous = editor
    const next = mutator(previous)
    if (next === previous) return
    // Only snapshot content-changing mutations for undo; pure cursor moves skip.
    if (getEditorText(next) !== getEditorText(previous)) {
      editorUndo.push(previous)
    }
    editor = next
  }

  function undoEditor(): boolean {
    const previous = editorUndo.pop()
    if (!previous) return false
    editor = previous
    return true
  }
  const compactLines: OutputLine[] = []
  const expandedLines: OutputLine[] = []
  const fileCompletion = new FileCompletion(completeAtFile)
  resources.add(() => fileCompletion.dispose())
  const screenLog = new ScreenLog()
  const committer = new Committer({
    compactLines,
    expandedLines,
    isExpanded: () => expanded,
    columns: () => renderer.termCols,
    logLines: lines => screenLog.logLines(lines),
    requestRender: () => renderer.requestRender(),
    invalidateHistory: () => resetHistoryCache(),
  })
  let liveContentMaxHeight = 0
  let liveContentWidth = renderer.termCols
  /**
   * First row of the redrawable region: everything above it is committed
   * transcript. Recorded by `buildFrame` so a keypress can release a native
   * terminal selection across the whole composer, not just the caret row.
   */
  let liveRegionStartRow = 0

  // Server state
  let serverState: ServerState | null = null
  try {
    serverState = await tryStartServer(opts.serverPort, opts.envFile)
  } catch { /* server start failed — continue without it */ }

  // Paste ref state
  const pastedChunks = new Map<number, string>()
  const pastedImages = new Map<number, { id: number; base64: string; mediaType: string; filePath?: string }>()
  let nextPasteId = 1

  // Update info
  let updateAvailable: { version: string } | null = null
  let updateStatus: 'idle' | 'downloading' | 'staged' = 'idle'
  let updateVersion: string | null = null
  const updateMgr = new (await import('../update/index.js')).UpdateManager(
    appVersion
  )
  updateMgr.on('update-available', (info: { version: string }) => {
    updateAvailable = { version: info.version }
    renderer.requestRender()
  })
  updateMgr.on('update-status', (status: { kind: 'idle' | 'downloading' | 'staged'; version?: string }) => {
    updateStatus = status.kind
    updateVersion = status.version ?? null
    renderer.requestRender()
  })
  {
    // A download staged by a previous session must show before the first
    // network check runs, so seed from disk alongside the manager's own read.
    const initial = updateMgr.getStatus()
    if (initial.kind !== 'idle') {
      updateStatus = initial.kind
      updateVersion = initial.version
    }
  }
  updateMgr.start()

  // Install bookkeeping. bin/evot reports its own version but the napi bindings
  // report nothing, so a half-finished install is otherwise silent until a
  // binding fails to load. Checked once at startup; never blocks.
  let installDrift: string | null = null
  try {
    const { checkInstallHealth } = await import('../update/state.js')
    const health = checkInstallHealth(appVersion)
    if (health.kind === 'drift') installDrift = health.reason
    // A newer version is already installed on disk while this session keeps the
    // image it started with. That is the staged-update state as far as the user
    // is concerned — reuse the restart notice instead of warning about a
    // mismatch and sending them to /update, which would reinstall for nothing.
    if (health.kind === 'restart_required') {
      updateStatus = 'staged'
      updateVersion = health.installedVersion
    }
  } catch { /* best effort */ }

  const historyMgr = new HistoryManager(agent.cwd)
  const inputImageHistory = new InputImageHistory()
  const entries = historyMgr.load().map(text => inputImageHistory.deserialize(text, () => nextPasteId++))
  historyState = createHistoryState(entries)

  let configInfo: ConfigInfo | undefined
  let cloudLoginRequired = false
  let revocationCleanup: Promise<void> | null = null
  let authWatcher: AuthWatcher | null = null
  const refreshConfigInfo = () => {
    // Re-read backend config after a model switch so the footer reflects the
    // new provider's effective thinking level (it can differ per provider).
    try { configInfo = agent.configInfo() } catch {}
  }
  refreshConfigInfo()

  const premiumAccount = hasPremiumModel(configInfo)
  adSlot = createAdSlotState(
    campaignContent(authNotices()),
    { premium: premiumAccount },
  )
  // Logged-in users see the slot from the start — no need to wait for a
  // task to finish. The live sync below refreshes content first.
  if (adSlot.notices.length > 0 || adSlot.ads.length > 0) {
    triggerAdSlot(adSlot, Date.now())
  }

  /** Single cloud-session status line; updates replace it in place. */
  function commitCloudSession(text: string, tone: 'dim' | 'ok' | 'warn'): void {
    const paint = tone === 'ok' ? chalk.green : tone === 'warn' ? chalk.yellow : chalk.dim
    commitStatusLine({ id: 'sys-cloud-session', kind: 'system', text: paint(text) })
  }

  /**
   * Re-resolve the model after an auth change and report whether one remains.
   * The backend keeps a still-served selection, so this only announces a model
   * the reload actually moved.
   */
  function reloadAfterAuthChange(): boolean {
    const previousSpec = currentModelSpec(configInfo, appState.model)
    const hasModel = agent.reloadSelection()
    refreshConfigInfo()
    reloadCloudContent()
    if (!hasModel) return false

    appState = { ...appState, model: agent.model }
    const next = configInfo?.availableModels.find(model => model.spec === currentModelSpec(configInfo, agent.model))
    if (next && next.spec !== previousSpec) {
      commitStatusLine({
        id: 'sys-model',
        kind: 'system',
        text: `  Model → ${formatModelLabel(agent.model, next.provider, next.group_label)}`,
      })
    }
    return true
  }

  function handleCloudSessionRevoked(): void {
    // A dead scoped key is recoverable: the catalog mints a fresh one on read,
    // so only a refused CLI token needs /login.
    if (revocationCleanup) return
    commitCloudSession('  ⟳ Cloud session key expired · restoring', 'dim')
    revocationCleanup = (async () => {
      try {
        const { authRefreshSession } = await import('../native/index.js')
        const { planAfterRevocation } = await import('../commands/login-flow.js')
        const plan = planAfterRevocation(await authRefreshSession())
        if (plan.kind === 'recovered') {
          cloudLoginRequired = !reloadAfterAuthChange()
          authWatcher?.sync()
          commitCloudSession('  ✓ Cloud session restored · send your message again', 'ok')
        } else if (plan.kind === 'unavailable') {
          // An outage says nothing about the credential; keep it.
          cloudLoginRequired = false
          commitCloudSession(`  ⚠ Cannot reach the evot server${plan.error ? ` (${plan.error})` : ''} · try again shortly`, 'warn')
        } else {
          cloudLoginRequired = true
          try { reloadAfterAuthChange() } catch { /* no provider left; /login is the fix */ }
          authWatcher?.sync()
          commitCloudSession('  ⚠ Cloud session signed out · run /login to reconnect', 'warn')
        }
      } catch (err) {
        cloudLoginRequired = true
        commitCloudSession(`  ⚠ Could not restore the cloud session: ${errorText(err)}`, 'warn')
      } finally {
        revocationCleanup = null
        renderer.requestRender()
      }
    })()
  }

  function queryBlockedByCloudLogin(): boolean {
    if (revocationCleanup) {
      commitCloudSession('  ⟳ Cloud session key expired · restoring', 'dim')
      return true
    }
    if (!cloudLoginRequired) return false
    commitCloudSession('  ⚠ Cloud session signed out · run /login to reconnect', 'warn')
    return true
  }

  function activeProviderIsCloud(): boolean {
    const provider = configInfo?.provider
    if (!provider) return false
    const active = configInfo?.availableModels.find(
      option => option.provider === provider && option.model === appState.model,
    )
    return active !== undefined && isCloudModel(active)
  }

  if (shouldPreloadStartupSessions(opts)) {
    try {
      preloadedSessions = await agent.listSessions(opts.continueLatest ? 0 : 20)
      resumeCache.replace(preloadedSessions, Boolean(opts.continueLatest))
    } catch {}
  }

  /** Drop snapshots after a run changes session visibility, title, or recency. */
  function invalidateResumeSessionCache(): void {
    resumeCache.invalidate()
    enrichedResumeMetadataGeneration = null
    enrichedResumeTextGeneration = null
    cancelSearchTextWarm()
  }

  function invalidateExplicitResumeSelector(): void {
    explicitResumeSelectorGeneration++
  }

  // Git info is watched so the footer follows external `git switch` / checkout
  // operations without requiring a REPL restart.
  const gitInfo = new GitInfoProvider(agent.cwd)
  gitInfo.onChange(() => renderer.requestRender())

  setTerminalTitle('✳')

  if (opts.continueLatest) {
    const match = findPreviousSession(preloadedSessions, agent.cwd)
    if (match) {
      await resumeSession(match)
    } else {
      commitSystem('sys-continue-err', chalk.red('No conversation found to continue'))
      cleanup()
      await sessionHook.close()
      fastExit(1)
    }
  } else if (opts.resumeSessionId) {
    const match = preloadedSessions.find(
      (s) => s.session_id === opts.resumeSessionId || s.session_id.startsWith(opts.resumeSessionId!)
    )
    if (match) {
      await resumeSession(match)
    } else {
      commitSystem('sys-resume-err', chalk.red(`Session not found: ${opts.resumeSessionId}`))
    }
  }

  renderer.requestRender()

  function getPromptVM(): PromptVMInput {
    return promptFromSnapshot({
      editor,
      session: appState,
      config: configInfo,
      active: overlay.kind === 'none',
      caretVisible: caretBlink.visible,
      planning,
      logMode: logMode !== null,
      dashboardUrl: serverState?.address ?? null,
      exitHint,
      columns: renderer.termCols,
      rows: renderer.termRows,
      gitBranch: gitInfo.getBranch(),
      backgroundProcessCount: backgroundTerminals.runningCount(),
    })
  }

  // Release notes (shown once after update)
  let releaseNotes: string[] | null = null
  try {
    const { markReleaseNotesSeen, releaseNotesPending } = await import('../update/seen-version.js')
    if (releaseNotesPending(appVersion)) {
      const { parseReleaseNotes } = await import('../update/notes.js')
      const { fetchReleaseNotesFor } = await import('../update/check.js')
      // Notes for the build that is running, not whatever is newest. Keep the
      // version pending when offline; only a matching release record proves the
      // metadata was handled and may be marked seen.
      fetchReleaseNotesFor(appVersion).then((info) => {
        if (!info) return
        markReleaseNotesSeen(appVersion)
        if (info.body) {
          releaseNotes = parseReleaseNotes(info.body)
          renderer.requestRender()
        }
      }).catch(() => {})
    }
  } catch { /* best effort */ }

  const bannerCache = new BannerCache(agent.cwd, agent.skillsDirs())
  function refreshBannerData(): void {
    if (bannerCache.refresh(agent.cwd, agent.skillsDirs())) renderer.requestRender()
  }
  // Catch edits made by other sessions without filesystem work in buildFrame.
  const bannerRefreshTimer = setInterval(refreshBannerData, 15_000)
  resources.add(() => clearInterval(bannerRefreshTimer))
  bannerRefreshTimer.unref?.()

  function currentBannerText(): string {
    return bannerCache.render({
      version: appVersion,
      model: agent.model,
      cwd: agent.cwd,
      configInfo,
      columns: renderer.termCols,
      rows: renderer.termRows,
      serverState,
      releaseNotes,
      installDrift,
    })
  }

  // --- buildFrame: the single render callback for the new differential renderer ---
  // Live-partial memo: spinner ticks (10/s) and keystrokes repaint the frame
  // without changing the assistant content. The reducer replaces the content
  // array on every real change, so reference equality is an exact dirty check
  // and pure-repaint frames skip the Markdown pipeline entirely.
  const EMPTY_ASSISTANT_CONTENT: UIAssistantBlock[] = []
  let partialBlocksMemo: {
    content: UIAssistantBlock[]
    expanded: boolean
    streaming: boolean
    columns: number
    blocks: ViewBlock[]
  } | null = null

  function buildPartialAssistantBlocks(): ViewBlock[] {
    const content = streamMachine?.appState.currentAssistantContent ?? EMPTY_ASSISTANT_CONTENT
    // Only provider deltas are provisional. The entire partial message stays in
    // the dynamic zone and is reparsed on every content delta, matching pi: a
    // growing table can update its rows and column geometry through line diffs.
    const streaming = spinnerState.streaming
    const columns = renderer.termCols
    if (
      partialBlocksMemo
      && partialBlocksMemo.content === content
      && partialBlocksMemo.expanded === expanded
      && partialBlocksMemo.streaming === streaming
      && partialBlocksMemo.columns === columns
    ) {
      return partialBlocksMemo.blocks
    }
    // Use the exact ordered committed-output pipeline for the live partial. This
    // keeps thinking/text/tool positions, margins, and prefixes stable through
    // completion instead of rendering tool calls in a detached layer.
    const blocks = buildOutputBlocks(assistantMessageToOutputLines(content, expanded, {
      streaming,
    }), {
      columns,
    })
    partialBlocksMemo = { content, expanded, streaming, columns, blocks }
    return blocks
  }

  function buildFrame(): RenderFrame {
    if (destroyed) return { lines: [] }

    // An overlay owns the screen, so hold the caret solid rather than
    // animating behind a modal.
    caretBlink.setEnabled(overlay.kind === 'none')

    const blocks: ViewBlock[] = []

    // 1. Banner
    const banner = currentBannerText()
    if (banner) {
      blocks.push({ lines: banner.split('\n').map(l => ({ spans: [{ text: l }] })), marginTop: 0 })
    }

    // 2. History (committed output lines) — incrementally cached so the
    // high-frequency spinner/delta/keystroke renders skip re-flattening the
    // whole transcript. The cache extends in place on append and rebuilds only
    // on reset (clear/replace), width change, or shrink. See HistoryRenderCache.
    const cols = renderer.termCols
    if (cols !== liveContentWidth) {
      liveContentWidth = cols
      liveContentMaxHeight = 0
    }
    const cache = expanded ? expandedHistoryCache : compactHistoryCache
    const cachedHistoryLines = cache.sync(expanded ? expandedLines : compactLines, cols)
    if (cachedHistoryLines.length > 0) {
      blocks.push({ lines: cachedHistoryLines.map(l => ({ spans: [{ text: l }] })), marginTop: 0 })
    }

    // 3. Ordered partial assistant message (thinking/text/tool calls). Markdown
    // prefixes can legitimately reparse into fewer rows as a fence/list/table
    // becomes complete. Track history + partial as one region: when completion
    // moves the same content from partial into history its total height remains
    // continuous, and any transient parser shrink is absorbed above the footer.
    const partialBlocks = buildPartialAssistantBlocks()
    blocks.push(...partialBlocks)
    const livePartialHeight = blocksToLines(partialBlocks).length
    const liveContentHeight = cachedHistoryLines.length + livePartialHeight
    // The monotonic-height guard is only needed while visible partial content is
    // being reparsed. At the start of a fresh LLM call currentAssistantContent is
    // empty; retaining the previous call's peak then creates up to eight literal
    // blank rows above Thinking…. Reset immediately until the first visible block.
    const liveHeight = updateLiveHeight(
      liveContentMaxHeight,
      liveContentHeight,
      isLoading && livePartialHeight > 0,
    )
    liveContentMaxHeight = liveHeight.maxHeight
    if (liveHeight.padding > 0) {
      blocks.push({
        lines: Array.from({ length: liveHeight.padding }, () => ({ spans: [{ text: '' }] })),
        marginTop: 0,
      })
    }

    const latestContentLines = blocksToLines(blocks)
    const commandWindowIsMounted = commandWindowMounted()
    if (
      commandWindowIsMounted
      && (commandWindowContentLines === null || commandWindowContentWidth !== cols)
    ) {
      // Background commits stay frozen while the command window is mounted,
      // but a resize must rebuild wrapping before the renderer performs its
      // unavoidable width-change redraw.
      commandWindowContentLines = latestContentLines
      commandWindowContentWidth = cols
    }
    const contentLines = commandWindowIsMounted
      ? commandWindowContentLines ?? latestContentLines
      : latestContentLines
    // Everything below the committed transcript is repaintable, whichever
    // branch below builds it. A mounted command window freezes this boundary,
    // so late history/status commits cannot move its selector or composer.
    liveRegionStartRow = contentLines.length
    const toolCalls = assistantToolCalls(streamMachine?.appState.currentAssistantContent ?? [])
    let spinnerBlock: ViewBlock | null = null
    // pi keeps statusContainer before editorContainer, so the active-run status
    // remains visible even while a selector replaces the editor.
    if (isLoading && overlay.kind !== 'ask-user') {
      const usagePending = streamMachine?.activeLlmCall ?? false
      const liveOutputTokens = usagePending && toolCalls.length === 0 ? spinnerState.tokenCount : 0
      // Usage arrives only when the provider completes this call. During an
      // active call, show only its live output estimate; retaining the previous
      // call here would present stale cache/input values as if they were current.
      const compactToolName = manualCompactionPhase === 'remote'
        ? 'compact_remote'
        : manualCompactionPhase === 'local_fallback'
          ? 'compact_local_fallback'
          : manualCompactionPhase === 'local'
            ? 'compact_local'
            : 'compact'
      const spinnerForDisplay = manualCompaction.active
        ? { ...spinnerState, toolName: compactToolName }
        : spinnerState
      const spinnerText = formatSpinnerLine(
        spinnerForDisplay,
        Date.now(),
        // Manual compaction and local commands report no per-call usage of their
        // own; showing the previous run's tokens would misattribute them.
        manualCompaction.active || foregroundCommand
          ? undefined
          : spinnerStatsFromLastUsage(
              appState.currentRunStats.lastLlmUsage,
              liveOutputTokens,
              usagePending,
            ),
        {
          interruptible: foregroundCommand === null,
          model: appState.model,
          // Ctrl+B can move this work aside without killing it, so the hint
          // advertises that key alongside esc. Esc itself always interrupts.
          backgroundable: backgroundTerminals.foregroundCount() > 0,
          releaseWait: backgroundTerminals.foregroundCount() === 0 && backgroundTerminals.blockingWaitCount() > 0,
          interruptPending: interruptConfirmation.pending(interruptOwner()),
        },
      )
      spinnerBlock = {
        lines: wrapTextWithAnsi(spinnerText, renderer.termCols).map(text => ({ spans: [{ text }] })),
        // Separating blank above the status row; queue rows bring their own.
        marginTop: 1,
      }
    } else if (backgroundWaitSince !== null && overlay.kind !== 'ask-user') {
      // Idle, but a detached task is still running and will wake the agent when
      // it finishes. Without a status row here the transcript looks finished, so
      // a user watching a long build would conclude the work had stopped.
      //
      // Elapsed is measured from when the wait began, not from the last run, so
      // the clock reads as the age of the wait itself.
      const waitText = formatSpinnerLine(
        { ...backgroundWaitSpinner, phaseStartedAt: backgroundWaitSince },
        Date.now(),
        // No per-call usage belongs to a wait: nothing is being spent while the
        // agent is parked.
        undefined,
        { model: appState.model },
      )
      spinnerBlock = {
        lines: wrapTextWithAnsi(waitText, renderer.termCols).map(text => ({ spans: [{ text }] })),
        marginTop: 1,
      }
    }

    // Match pi's sibling order before editorContainer: pending messages, then
    // status. The queue manager suppresses the duplicate pending-message copy
    // because the selector itself is displaying those same entries.
    // One lifecycle tick and one optional wakeup per frame. Static text sleeps
    // until its rotation deadline instead of repainting at animation cadence.
    const adSlotNow = Date.now()
    const adSlotTick = tickAdSlot(adSlot, adSlotNow)
    adSlotWakeup.replace(nextAdSlotRenderDelay(adSlot, adSlotTick, adSlotNow,
      overlay.kind === 'none' && !isLoading && !spinnerBlock
      && !commandWindowIsMounted && renderer.termCols >= 30))
    const preEditorBlocks: ViewBlock[] = []
    const queueManagerOpen = overlay.kind === 'selector' && overlay.state.owner === SELECTOR_OWNER.queue
    const queueLines = queueManagerOpen
      ? []
      : formatQueuedMessageLines([
          ...queuedUserMessages.map(message => message.text),
          ...queuedCompactionSubmissions.map(message => message.displayText),
        ])
    if (queueLines.length > 0) {
      preEditorBlocks.push({
        lines: queueLines.map(text => ({ spans: [{ text, dim: true }] })),
        marginTop: 1,
      })
      // Queue already owns the blank line above the input unit.
      if (spinnerBlock) spinnerBlock = { ...spinnerBlock, marginTop: 0 }
    }
    // Update notice: sits on the same row as the spinner so a long wait
    // never grows an extra blank line above the composer. Staged is green
    // because it wants to be seen: "restart when convenient". Downloading
    // and manual-only-available are dim background noise. Failures stay
    // silent; the manual /update reports them with full context.
    let updateNotice = updateNoticeSpans({
      status: updateStatus,
      version: updateVersion,
      availableVersion: updateAvailable?.version ?? null,
      visible: overlay.kind === 'none' && !commandWindowIsMounted,
      busy: spinnerBlock !== null,
    })
    if (spinnerBlock && updateNotice) {
      spinnerBlock = attachUpdateNotice(spinnerBlock, updateNotice, renderer.termCols)
      updateNotice = null
    }
    if (spinnerBlock) preEditorBlocks.push(spinnerBlock)
    // The ad slot is an idle-time surface: hidden while a task is running
    // (spinner visible) so it never competes with live output.
    if (adSlotTick.content && !isLoading && !spinnerBlock && !commandWindowIsMounted) {
      preEditorBlocks.push(...buildAdSlotBlocks(adSlot, adSlotTick, renderer.termCols, adSlotNow))
    }
    if (updateNotice) {
      preEditorBlocks.push(standaloneUpdateNotice(updateNotice, renderer.termCols, preEditorBlocks.length > 0))
    }

    return buildShellFrame({
      contentLines,
      preEditorBlocks,
      prompt: getPromptVM(),
      overlay,
      commandFocused: focusedCommandWindowGeneration !== null,
      preview: commandWindowPreview,
    })
  }

  renderer.setRenderCallback(buildFrame)

  function outputContextFor(lines: OutputLine[]): { prevKind?: string; columns?: number } {
    return committer.contextFor(lines)
  }

  function restoreLines(outputLines: OutputLine[], expandedOutputLines: OutputLine[] = outputLines) {
    committer.restore(outputLines, expandedOutputLines)
  }

  function commitLines(outputLines: OutputLine[]) {
    committer.commit(outputLines)
  }

  function commitSystem(id: string, text: string, kind: OutputLine['kind'] = 'system') {
    committer.system(id, text, kind)
  }

  /**
   * Show a secret on screen, then take it back.
   *
   * The mechanics live on the Committer, which owns the line arrays and the
   * timer, so the erase is reachable by tests. Here this is only the wiring the
   * command layer calls through.
   */
  function commitRevealed(id: string, text: string, erasedText: string, delayMs: number) {
    committer.revealTemporarily(id, text, erasedText, delayMs)
  }

  /** Commit a transient status line (model / thinking level). Rapid re-toggles
   *  replace the previous status in place instead of stacking a new line each
   *  time. Model and thinking share one status slot so alternating switches
   *  stay single-line. Only the trailing line is eligible for replacement, so a
   *  later user message or other output freezes the prior status into history. */
  function commitStatusLine(line: OutputLine) {
    const replaced = replaceOrPushStatusLine(compactLines, line)
    replaceOrPushStatusLine(expandedLines, line)
    // In-place mutation invalidates the append-only history cache prefix.
    if (replaced) resetHistoryCache()
    const context = outputContextFor(compactLines.slice(0, -1))
    const blocks = buildOutputBlocks([line], context)
    const rendered = blocksToLines(blocks)
    screenLog.logLines(rendered)
    renderer.requestRender()
  }

  /** Commit slash-command system lines, collapsing model/thinking status in place. */
  function commitSystemLines(outputLines: OutputLine[]) {
    for (const line of outputLines) {
      if (line.kind === 'system' && (line.id === 'sys-model' || line.id === 'sys-think')) {
        commitStatusLine(line)
      } else {
        commitLines([line])
      }
    }
  }

  /** Commit flush result with optional dual-commit (compact summary vs expanded full). */
  function commitFlushResult(flushed: { lines: OutputLine[]; expandedLines?: OutputLine[] }) {
    if (flushed.lines.length === 0) return
    if (flushed.expandedLines) {
      compactLines.push(...flushed.lines)
      expandedLines.push(...flushed.expandedLines)
      const visible = expanded ? flushed.expandedLines : flushed.lines
      const context = outputContextFor(compactLines.slice(0, -flushed.lines.length))
      const blocks = buildOutputBlocks(visible, context)
      const rendered = blocksToLines(blocks)
      screenLog.logLines(rendered)
      renderer.requestRender()
    } else {
      commitLines(flushed.lines)
    }
  }

  /** Toggle expanded view and redraw. */
  function toggleExpanded(): void {
    expanded = !expanded
    // An explicit Ctrl+O layout change should take effect immediately rather
    // than being mistaken for parser-induced shrink by the live-height guard.
    liveContentMaxHeight = 0
    // Differential render, not a forced clear. When the content being toggled
    // (e.g. the tool output you just ran) sits in the viewport, the renderer
    // repaints in place from the first changed line down, so the view stays
    // put instead of clearing and re-anchoring to the bottom (which is what
    // made the screen jump). A swap large enough to change history above the
    // viewport still falls back to a full redraw via the renderer's own
    // off-viewport guard. Mirrors pi, which toggles with requestRender().
    renderer.requestRender()
  }

  /** Cycle the model's reasoning effort (Shift+Tab) and reflect it in the footer. */
  function cycleThinkingLevel(): void {
    let level: string | null
    try {
      level = agent.cycleThinkingLevel()
    } catch {
      return
    }
    if (level === null) {
      commitStatusLine({ id: 'sys-think', kind: 'system', text: '  This model has no selectable thinking level' })
      return
    }
    refreshConfigInfo()
    const label = level === 'off' ? 'off' : level
    commitStatusLine({ id: 'sys-think', kind: 'system', text: `  Thinking level → ${label}` })
    renderer.requestRender()
  }

  let titleFrame = 0
  const TITLE_INTERVAL_FRAMES = Math.round(960 / SPINNER_INTERVAL_MS) // ~960ms like Claude Code

  function startSpinner() {
    if (spinnerTimer) return
    titleFrame = 0
    spinnerTimer = setInterval(() => {
      spinnerState = advanceSpinner(spinnerState)
      if (manualCompaction.active) manualCompactionPhase = manualCompaction.phase
      if (streamMachine) {
        streamMachine = { ...streamMachine, spinnerState }
      }
      renderer.requestRender()
      // Terminal title animation — update at ~960ms like Claude Code.
      if (spinnerState.frame % TITLE_INTERVAL_FRAMES === 0) {
        const glyphs = ['⠂', '⠐']
        const idx = titleFrame % glyphs.length
        titleFrame++
        setTerminalTitle(glyphs[idx])
      }
    }, SPINNER_INTERVAL_MS)
  }

  function stopSpinner() {
    interruptConfirmation.clear()
    if (spinnerTimer) {
      clearInterval(spinnerTimer)
      spinnerTimer = null
    }
    // Always replace the final animated glyph. An ask overlay can keep the
    // title frozen while the run settles; normal title writes are correctly
    // blocked then, but the completed state must not remain stuck on ·/⠂/⠐.
    setTerminalTitle(backgroundWaitSince !== null ? '◌ bg' : '✳', true)
  }

  async function resumeSession(session: SessionMeta) {
    try {
      const { transcript, model, provider, thinkingLevel, cwd: sessionCwd } = await prepareResume(agent, session)

      // Restore model selection from current config, then the session's
      // recorded thinking level (session wins over the config default).
      // Missing saved selections keep the refreshed live model and show a
      // recovery hint.
      const modelRestoreNote = reloadResumeModel(agent, model, provider, thinkingLevel)
      sessionId = session.session_id
      sessionHook.startSession(session.session_id, agent.cwd)
      sessionHook.state('idle')
      rendererTrace.bind(session.session_id)
      refreshConfigInfo()
      appState = { ...appState, sessionId: session.session_id, model: agent.model }
      const { messagesToOutputLines } = await import('../render/output.js')
      const { transcriptToMessages } = await import('../session/transcript.js')
      const messages = transcriptToMessages(transcript)
      // A resumed session starts with no active plan; plan mode is re-entered
      // via /plan on the live conversation.
      planModeItems = []
      lastReviewedPlanMarkdown = ''
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      resetHistoryCache()
      // Only render the most recent messages to scrollback. Rendering the whole
      // transcript re-runs markdown (marked lex + ANSI + table align) per
      // message, which is O(total) and reaches ~500ms on very long sessions.
      // The hidden messages stay in the model's context (the backend restores
      // it by session_id independently of this display transcript), so this
      // only trims what's painted, not what the model remembers.
      const { shown, hidden } = selectResumeMessages(messages)
      if (hidden > 0) restoreLines([resumeElidedLine(hidden)])
      restoreLines(messagesToOutputLines(shown), messagesToOutputLines(shown, true))
      restoreLines([
        { id: 'sys-resumed-gap', kind: 'system', text: '' },
        { id: 'sys-resumed', kind: 'system', text: chalk.dim(`  resumed session ${session.session_id.slice(0, 8)}`) },
      ])
      if (sessionCwd && sessionCwd !== agent.cwd) {
        restoreLines([{
          id: 'sys-resume-cwd',
          kind: 'system',
          text: chalk.dim(`  session cwd: ${shortenSessionCwd(sessionCwd)} · working cwd remains: ${shortenSessionCwd(agent.cwd)}`),
        }])
      }
      if (modelRestoreNote) {
        restoreLines([{ id: 'sys-resume-model', kind: 'system', text: chalk.dim(modelRestoreNote) }])
      }
    } catch (err) {
      commitSystem('sys-err', `Failed to resume: ${errorText(err)}`, 'error')
    }
  }

  async function rebuildAfterManualCompaction(outcome: Extract<ManualCompactionOutcome, { status: 'compacted' }>) {
    if (!sessionId) return
    const transcript = await agent.loadContextTranscript(sessionId)
    const messages = transcriptToMessages(transcript).filter(message =>
      !(message.role === 'user' && message.text.startsWith(COMPACT_SUMMARY_PREFIX)),
    )
    const { shown, hidden } = selectResumeMessages(messages)

    appState = {
      ...appState,
      messages,
      sessionTokens: {
        ...appState.sessionTokens,
        contextTokens: outcome.tokens_after,
        contextWindow: outcome.context_window || appState.sessionTokens.contextWindow,
      },
    }
    renderer.clearScreen()
    compactLines.length = 0
    expandedLines.length = 0
    resetHistoryCache()
    if (hidden > 0) restoreLines([resumeElidedLine(hidden)])
    restoreLines(messagesToOutputLines(shown))

    const lines = manualCompactionLines(outcome)
    compactLines.push(...lines.compact)
    expandedLines.push(...lines.expanded)
    renderer.requestRender()
  }

  async function submitQueuedAfterCompaction() {
    const submissions = queuedCompactionSubmissions
    queuedCompactionSubmissions = []
    for (const submission of submissions) {
      commitLines(buildUserMessage(submission.displayText))
      await runQuery(submission.expandedText, submission.contentJson)
    }
  }

  async function runManualCompaction(customInstructions: string) {
    if (!sessionId) {
      commitSystem('sys-compact', '  Nothing to compact: no active session.')
      return
    }

    isLoading = true
    spinnerState = setSpinnerPhase(createSpinnerState(), 'executing', 'compact')
    startSpinner()
    const targetSession = sessionId
    try {
      await manualCompaction.run(
        () => agent.compact(targetSession, customInstructions || undefined),
        () => {
          manualCompactionPhase = manualCompaction.phase
          renderer.requestRender()
        },
        async outcome => {
          if (outcome.status === 'compacted') {
            await rebuildAfterManualCompaction(outcome)
          } else if (outcome.status === 'cancelled') {
            commitSystem('sys-compact-cancelled', '  Compaction cancelled.')
          } else {
            commitSystem('sys-compact-empty', '  Nothing to compact.')
          }
        },
      )
    } catch (err) {
      commitSystem('sys-compact-err', `Compact failed: ${errorText(err)}`, 'error')
    } finally {
      manualCompactionPhase = null
      isLoading = false
      stopSpinner()
      renderer.requestRender()
    }
    await submitQueuedAfterCompaction()
  }

  /** Get expanded text — resolves paste refs, strips only resolved image refs. */
  function getExpandedText(resolvedImageIds?: Set<number>): string {
    return resolveSubmitText(getEditorText(editor), pastedChunks, resolvedImageIds ?? null)
  }

  /** Get display text (raw with refs intact). */
  function getDisplayText(): string {
    return getEditorText(editor).trim()
  }

  /** Get history text: expand text pastes before their in-memory store is cleared. */
  function getHistoryText(): string {
    const text = resolveHistoryText(getEditorText(editor), pastedChunks)
    inputImageHistory.capture(text, pastedImages)
    return text
  }

  function saveInputHistory(text: string): void {
    historyMgr.append(inputImageHistory.serialize(text))
    historyState = pushHistory(historyState, text)
  }

  /** Clear editor and paste state. */
  function clearAll() {
    cancelResumeCommandLoad()
    cancelResumeSearchEnrichment()
    editor = clearEditor(editor)
    editorUndo.clear()
    pastedChunks.clear()
    pastedImages.clear()
  }

  /** Unique id per emission: these lines are history, not collapsible status. */
  function commitBackgroundLine(slot: string, text: string): void {
    commitSystem(`sys-bg-${slot}-${nextBackgroundLineId++}`, text)
  }

  const backgroundTerminals = new BackgroundTerminals({
    client: agent,
    sessionId: () => sessionId,
    commit: commitBackgroundLine,
    requestRender: () => renderer.requestRender(),
    errorText,
    paintError: text => chalk.red(text),
    readOutput: path => readOutputTail(path),
    watchOutput: watchOutputFile,
    openPanel: state => {
      overlay = { kind: 'selector', state }
      renderer.requestRender()
    },
    updatePanel: state => {
      // Only ever touch the overlay while the panel owns it: an async stop
      // landing after the user moved on must not close a different overlay.
      if (overlay.kind !== 'selector' || !isBackgroundSelector(overlay.state)) return
      overlay = state ? { kind: 'selector', state } : { kind: 'none' }
      renderer.requestRender()
    },
    panelOpen: () => overlay.kind === 'selector' && isBackgroundSelector(overlay.state),
    panelState: () =>
      overlay.kind === 'selector' && isBackgroundSelector(overlay.state) ? overlay.state : null,
    // A run drains the notification queue itself between turns, so waking is
    // only for the idle case.
    runInFlight: () => isLoading,
    queuedMessages: () => queuedUserMessages.length + queuedCompactionSubmissions.length,
    // An ask overlay is the agent waiting on the user: seizing the turn would
    // answer a question they have not answered yet.
    overlayBlocking: () => overlay.kind === 'ask-user',
    wakeForNotifications: () => {
      // No text: build_turn puts the queued completion notices into this turn's
      // input, so a synthetic prompt would duplicate what the model just read.
      // Fire-and-forget because the poll cannot await; runQuery owns its own
      // lifecycle and errors from here.
      void runQuery('')
    },
  })

  backgroundCleanup.add(() => backgroundTerminals.killAllNow())

  function refreshBackgroundProcesses(): void {
    backgroundTerminals.refresh()
    // Track the idle wait alongside the poll that discovers it. A live
    // background task will wake the agent when it finishes, so while one is
    // running the agent is genuinely parked rather than done.
    const waiting = backgroundTerminals.runningCount() > 0
    if (waiting && backgroundWaitSince === null) {
      backgroundWaitSince = Date.now()
      renderer.requestRender()
    } else if (!waiting && backgroundWaitSince !== null) {
      backgroundWaitSince = null
      if (!isLoading) setTerminalTitle('✳')
      renderer.requestRender()
    }
  }

  // Animates the idle wait row. The spinner timer only runs during a turn, and
  // the ad-slot ticker stops whenever an overlay owns the screen, so neither can
  // be relied on to keep this glyph moving.
  const backgroundWaitTimer = setInterval(() => {
    if (isLoading || backgroundWaitSince === null) return
    backgroundWaitSpinner = advanceSpinner(backgroundWaitSpinner)
    if (backgroundWaitSpinner.frame % TITLE_INTERVAL_FRAMES === 0) {
      setTerminalTitle(`${['⠂', '⠐'][titleFrame++ % 2]} bg ${backgroundTerminals.runningCount()}`)
    }
    renderer.requestRender()
  }, SPINNER_INTERVAL_MS)
  ;(backgroundWaitTimer as unknown as { unref?: () => void }).unref?.()
  resources.add(() => clearInterval(backgroundWaitTimer))

  /** Insert pasted text, collapsing large pastes into refs. */
  function insertPaste(raw: string) {
    const cleaned = cleanPastedText(raw)
    if (shouldCollapse(cleaned)) {
      const id = nextPasteId++
      const numLines = (cleaned.match(/\n/g) || []).length
      pastedChunks.set(id, cleaned)
      const ref = formatPastedTextRef(id, numLines)
      mutateEditor(state => insertText(state, ref))
    } else {
      mutateEditor(state => insertText(state, cleaned))
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
      mutateEditor(state => insertText(state, formatImageRef(id)))
      renderer.requestRender()
    }
  }

  /** Paste clipboard contents (Cmd+V). Image wins when both are present. */
  async function tryPasteClipboard() {
    const img = await getImageFromClipboard()
    if (img) {
      await tryPasteImage()
      return
    }
    const text = await getTextFromClipboard()
    if (text) {
      insertPaste(text)
      renderer.requestRender()
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
      blocks.push({
        type: 'image',
        mimeType: img.mediaType,
        source: img.filePath
          ? { type: 'path', path: img.filePath }
          : { type: 'base64', data: img.base64 },
      })
    }
    return { blocks, resolvedIds: new Set(resolved.map(r => r.id)) }
  }

  async function runQuery(text: string, contentJson?: string, prebuiltStream?: QueryStream) {
    if (queryBlockedByCloudLogin()) return
    // Notification-only wakeups continue the same user task; don't hide its
    // newly completed background work when the engine reads the result.
    if (text || contentJson || prebuiltStream) backgroundTerminals.beginRun()
    const generation = beginRun()
    liveContentMaxHeight = 0
    isLoading = true
    spinnerState = createSpinnerState()
    streamMachine = createStreamMachineState(appState, spinnerState)
    startSpinner()
    renderer.requestRender()

    let completed = false
    try {
      const stream = prebuiltStream
        ?? await agent.query(text, sessionId ?? undefined, planning ? 'planning_interactive' : 'interactive', contentJson, HOST_TOOL_SPECS_JSON)
      if (!ownsRun(generation)) {
        stream.abort()
        return
      }
      streamRef = stream
      sessionId = stream.sessionId ?? sessionId
      if (sessionId) {
        sessionHook.startSession(sessionId, agent.cwd)
        // The query may have just persisted a formerly unbound first session,
        // and every run can change its title, turn count, and recency.
        invalidateResumeSessionCache()
      }
      appState = { ...appState, sessionId: sessionId }
      screenLog.bind(stream.sessionId)
      rendererTrace.bind(stream.sessionId)

      for await (const event of stream) {
        if (destroyed || !ownsRun(generation)) break
        if (!streamMachine) break

        if (isHostToolEvent(event)) {
          const response = await dispatchHostToolCall(event.payload, collectAskUserAnswers)
          if (ownsRun(generation)) await stream.respondHostTool(JSON.stringify(response))
          continue
        }

        if (event.kind === 'run_started') {
          sessionHook.startSession(event.session_id, agent.cwd)
          sessionHook.runStarted(event.run_id)
        } else if (event.kind === 'run_finished') {
          sessionHook.runFinished(event.run_id)
        } else if (event.kind === 'error') {
          sessionHook.runFailed(event.run_id, String(event.payload?.message ?? ''))
        }

        const update = reduceRunEvent(streamMachine!, event, {
          termRows: renderer.termRows,
          cloudProvider: activeProviderIsCloud(),
        })

        streamMachine = update.state
        appState = update.state.appState
        spinnerState = update.state.spinnerState
        if (update.sessionRevoked) handleCloudSessionRevoked()

        // Git commands run inside tool subprocesses. Refresh synchronously when
        // any tool settles instead of waiting for the debounced HEAD watcher;
        // otherwise the completed answer can still render the previous branch.
        if (event.kind === 'tool_finished') gitInfo.refresh()

        // Request re-render on each delta so streaming text appears
        if (event.kind === 'assistant_delta') {
          renderer.requestRender()
        }

        if (update.commitLines.length > 0) {
          if (update.expandedCommitLines) {
            // Dual-commit: compact in compactLines, expanded in expandedLines
            const compact = update.commitLines
            const exp = update.expandedCommitLines
            compactLines.push(...compact)
            expandedLines.push(...exp)
            const visible = expanded ? exp : compact
            const context = outputContextFor(compactLines.slice(0, -compact.length))
            const blocks = buildOutputBlocks(visible, context)
            const rendered = blocksToLines(blocks)
            screenLog.logLines(rendered)
            renderer.requestRender()
          } else {
            commitLines(update.commitLines)
          }
        }

        // Reconcile against the native queue instead of draining the visible
        // copy wholesale: OneAtATime mode may consume only the first of several
        // queued prompts at this boundary.
        if (event.kind === 'turn_started') reconcileQueuedUserMessages()

        // writeLines are log-only: LLM/COMPACT/SPILL stats that don't render in
        // the TUI. Run them through the same formatting pipeline so screen.log
        // still captures the observability detail for post-hoc debug.
        if (update.writeLines.length > 0) {
          const blocks = buildOutputBlocks(update.writeLines, { columns: renderer.termCols })
          const rendered = blocksToLines(blocks)
          screenLog.logLines(rendered)
        }

        if (update.rerenderStatus) renderer.requestRender()
      }

      if (!ownsRun(generation)) return
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        appState = final.state.appState
        commitFlushResult(final)
      }
      // Safety net: commit only prompts the engine actually consumed. A prompt
      // queued during the final poll can still be pending when the run settles.
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
      completed = true
    } catch (err) {
      // An interrupted run is expected to reject after its ownership has been
      // revoked. Its interruption notice was already committed synchronously;
      // touching shared state here could flush or clear a newer run.
      if (!ownsRun(generation)) return
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        commitFlushResult(final)
      }
      const message = errorText(err)
      commitSystem('sys-err', message, 'error')
      sessionHook.runFailed(undefined, message)
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
    } finally {
      if (ownsRun(generation)) {
        // Safety net for a run that ended without run_finished/error (e.g. the
        // stream just stopped). No-op when the run already settled above.
        sessionHook.settleRun()
        unfreezeTerminalTitle()
        streamRef = null
        isLoading = false
        streamMachine = null
        stopSpinner()
        // Fresh ads/models belong in the background: awaiting the catalog here
        // stalled the prompt for the whole HTTP round-trip after every turn.
        void syncCloudNow(true)
        triggerAdSlot(adSlot, Date.now())
        renderer.requestRender()
      }
    }

    if (!ownsRun(generation) || !completed) return

    // Final metadata is saved as the stream settles. Force the next /resume to
    // observe those values even if some background consumer repopulated a
    // snapshot while the run was active.
    invalidateResumeSessionCache()
    await maybeReviewPlanAfterTurn()
  }

  function handleKey(event: KeyEvent) {
    // Typing keeps focus in the composer while refreshing the formal window
    // above it. Up/down is the explicit gesture that promotes that same state.
    if (activateCommandWindow(event)) return
    handleKeyInner(event)
    refreshCommandWindowPreview(isCommandWindowTypingEvent(event))
  }

  function handleKeyInner(event: KeyEvent) {
    caretBlink.bump()

    // Mouse dragging creates a native terminal selection outside our editor
    // state. Most keypresses repaint the whole live region to release it. A
    // selector navigation key is the latency-sensitive exception: repainting
    // every selector, preview-pane and composer row made ↑/↓ feel sticky even
    // though only two choice rows changed. Let the renderer diff those rows.
    const commandSelectorNavigation = overlay.kind === 'selector'
      && focusedCommandWindowGeneration !== null
      && isCommandSelector(overlay.state)
      && (event.type === 'up' || event.type === 'down' || event.type === 'tab' || event.type === 'shift-tab')
    // While the composer owns a mounted command window, ordinary text edits
    // should likewise remain differential. Invalidating from the live-region
    // start reaches above the viewport for a tall session pane, forcing a
    // clear-and-repaint even when `/re` merely becomes `/`.
    const commandWindowEditing = commandWindowPreview !== null
      && (event.type === 'char'
        || event.type === 'shift-char'
        || event.type === 'backspace'
        || event.type === 'delete')
    if (!commandSelectorNavigation && !commandWindowEditing) {
      renderer.invalidateRowsFrom(liveRegionStartRow)
    }

    if (editingQueuedPrompt) {
      if (event.type === 'escape' || (event.type === 'ctrl' && event.key === 'c')) {
        cancelQueueEdit()
        return
      }
      if (isQueueManageShortcut(event)) {
        commitSystem('sys-queue-edit-lock', '  Finish or discard the queued prompt edit first.')
        return
      }
    }
    if (isQueueManageShortcut(event)) {
      if (overlay.kind === 'selector' && overlay.state.owner === SELECTOR_OWNER.queue) {
        overlay = { kind: 'none' }
        renderer.requestRender()
      } else if (streamRef && queuedUserMessages.length > 0) {
        openQueueSelector()
      }
      return
    }

    if (isBackgroundPanelShortcut(event)) {
      backgroundTerminals.togglePanel()
      return
    }
    // The panel claims its own gestures (enter / x / X / esc) before the
    // generic selector handling, which would otherwise treat them as filter
    // input or a plain selection.
    if (overlay.kind === 'selector' && isBackgroundSelector(overlay.state)) {
      if (backgroundTerminals.handlePanelKey(event)) return
    }

    const actions = decideReplControl({
      event,
      overlay,
      isLoading,
      hasStream: streamRef !== null,
      editor,
      exitHint,
      logMode: logMode !== null,
      hasQueuedPrompt: queuedUserMessages.length > 0,
      isCompacting: manualCompaction.active,
      canReclaimTurn: backgroundTerminals.canReclaimTurn(),
    })

    for (const action of actions) {
      if (applyReplControlAction(action, event)) return
    }
  }

  function applyReplControlAction(action: ReplControlAction, event: KeyEvent): boolean {
    switch (action.kind) {
      case 'restore-queued':
        restoreLastQueuedUserMessageToEditor()
        return true
      case 'reclaim-turn': {
        // Non-destructive by contract: ctrl+b never kills. If nothing was
        // actually released (it all finished in the same tick) say so rather
        // than falling through to an interrupt — this key must never be the one
        // that ends a run.
        const wasForeground = backgroundTerminals.foregroundCount() > 0
        const freed = backgroundTerminals.reclaimTurn()
        if (freed === 0) {
          commitSystem('sys-reclaim-turn', '  Nothing to move to the background.')
          renderer.requestRender()
          return true
        }
        if (wasForeground) {
          commitSystem('sys-reclaim-turn', '  ● Shell moved to background; it keeps running.')
        }
        renderer.requestRender()
        return true
      }
      case 'interrupt':
        // Same gesture as opencode: the first Esc arms a short window and the
        // spinner switches to "esc again to interrupt"; only the second press
        // within that window stops the work.
        if (!interruptConfirmation.press(interruptOwner())) {
          renderer.requestRender()
          return true
        }
        if (manualCompaction.active) {
          manualCompaction.abort()
          return true
        }
        interruptStream('sys-int', '  Interrupted.')
        return true
      case 'exit':
        cleanup()
        if (sessionId) {
          process.stdout.write(`\n\x1b[90m${'─'.repeat(80)}\x1b[0m\n`)
          process.stdout.write(`\x1b[90mResume: evot --resume ${sessionId}\x1b[0m\n\n`)
        }
        exitAfterCleanup(0)
        return true
      case 'show-exit-hint':
        exitHint = true
        renderer.requestRender()
        if (exitHintTimer) clearTimeout(exitHintTimer)
        exitHintTimer = setTimeout(() => { exitHint = false; renderer.requestRender() }, 2000)
        return true
      case 'clear-editor':
        editor = clearEditor(editor)
        renderer.requestRender()
        return true
      case 'close-completion':
        editor = closeCompletion(editor)
        renderer.requestRender()
        return true
      case 'clear-exit-hint':
        exitHint = false
        return false
      case 'cancel-ask':
        overlay = { kind: 'none' }
        unfreezeTerminalTitle()
        interruptStream('sys-ask-cancel', '  ⏺ Cancelled.')
        return true
      case 'clear-selector-query':
        if (overlay.kind === 'selector') overlay = { kind: 'selector', state: selectorClearQuery(overlay.state) }
        renderer.requestRender()
        return true
      case 'close-overlay': {
        // Closing a promoted command window also consumes its slash command,
        // matching the previous one-Esc lifecycle. The hot focus/navigation
        // path remains stable; only this explicit close is allowed to shrink.
        const closesCommandWindow = focusedCommandWindowGeneration !== null
          && overlay.kind === 'selector'
          && isCommandSelector(overlay.state)
        if (overlay.kind === 'ask-user') resolvePendingAsk()
        invalidateExplicitResumeSelector()
        overlay = { kind: 'none' }
        focusedCommandWindowGeneration = null
        cancelResumeSearchEnrichment()
        nextCommandWindowGeneration()
        if (closesCommandWindow) clearAll()
        releaseCommandWindowLayout()
        renderer.requestRender()
        return true
      }
      case 'exit-log-mode':
        logMode = null
        commitSystem('sys-log-exit', '  [log mode] exited')
        renderer.requestRender()
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
        if (event.type === 'char' || event.type === 'shift-char') {
          editor = insertText(editor, event.char)
          renderer.requestRender()
        }
        return true
      case 'loading-paste':
        if (event.type === 'paste') {
          insertPaste(event.text)
          renderer.requestRender()
        }
        return true
      case 'normal-key':
        handleNormalKey(event)
        return true
    }
  }

  /** Flush any in-progress streaming content to committed output.
   *  Call before clearing streaming state on any abort/cancel path. */
  function flushStreamContent() {
    // Flush anything the stream machine accumulated
    if (!streamMachine) return
    const flushed = flushStreaming(streamMachine)
    streamMachine = flushed.state
    commitFlushResult(flushed)
  }

  function interruptStream(id: string, text: string) {
    unfreezeTerminalTitle()
    // Revoke ownership before aborting: native abort settles asynchronously,
    // and its rejected Promise must not later emit a second Interrupted/error
    // or clear a newer query that the user starts immediately afterward.
    revokeRun()
    // If an ask/plan-review overlay is awaiting, resolve it as cancelled so the
    // suspended host-tool dispatch in runQuery unblocks instead of hanging the
    // run loop forever.
    resolvePendingAsk()
    const interruptedStream = streamRef
    streamRef = null
    interruptedStream?.abort()
    isLoading = false
    flushStreamContent()
    streamMachine = null
    stopSpinner()
    // Mid-stream queue was steered but the run is aborted — put it back in the
    // editor so the user can edit and press Enter, instead of committing it as
    // history under the cancellation notice.
    restoreQueuedUserMessagesToEditor()
    commitLines([{ id, kind: 'cancelled', text }])
    backgroundTerminals.parkUntilBackground()
    sessionHook.settleRun()
  }

  function managedQueueEntries(): ManagedQueuedPrompt[] {
    if (!streamRef) return []
    return visibleQueueEntries(readPromptQueues(streamRef), queuedUserMessages)
  }

  function openQueueSelector() {
    let entries: ManagedQueuedPrompt[]
    try {
      entries = managedQueueEntries()
    } catch (err) {
      commitSystem('sys-queue-err', `  Queue read failed: ${errorText(err)}`, 'error')
      renderer.requestRender()
      return
    }
    if (entries.length === 0) {
      overlay = { kind: 'none' }
      commitSystem('sys-queue-empty', '  No queued prompts.')
      return
    }
    overlay = { kind: 'selector', state: createQueueSelectorState(entries) }
    renderer.requestRender()
  }

  function editQueuedPrompt(entry: ManagedQueuedPrompt) {
    if (!streamRef) return
    editingQueuedPrompt = entry
    stashedQueueEditDraft = getEditorText(editor)
    clearAll()
    editor = insertText(editor, entry.text)
    overlay = { kind: 'none' }
    commitSystem('sys-queue-edit', '  Editing queued prompt · Enter save · Esc discard')
    renderer.requestRender()
  }

  function finishQueueEdit() {
    editingQueuedPrompt = null
    clearAll()
    editor = insertText(editor, stashedQueueEditDraft)
    stashedQueueEditDraft = ''
    renderer.requestRender()
  }

  function cancelQueueEdit() {
    finishQueueEdit()
    commitSystem('sys-queue-edit-cancel', '  Queue edit discarded.')
  }

  function saveQueueEdit(text: string) {
    if (!streamRef || !editingQueuedPrompt || !text.trim()) return
    const entry = editingQueuedPrompt
    try {
      const updated = streamRef.updateQueuedPrompt(entry.queue, entry.id, entry.version, text)
      queuedUserMessages = queuedUserMessages.map(message => message.id === entry.id
        ? { ...message, version: updated.version, text }
        : message)
      finishQueueEdit()
      commitSystem('sys-queue-edit-save', '  Queued prompt updated.')
    } catch (err) {
      try {
        const current = managedQueueEntries().find(candidate => candidate.id === entry.id)
        if (current) editingQueuedPrompt = { ...current, text }
        else finishQueueEdit()
      } catch {
        // Failed refresh is not proof the entry was consumed. Retain the edit
        // and its draft so the user can retry or explicitly discard it.
      }
      commitSystem('sys-queue-err', chalk.red(`  Queue edit failed: ${errorText(err)}`))
      renderer.requestRender()
    }
  }

  function removeQueuedPrompt(entry: ManagedQueuedPrompt) {
    if (!streamRef) return
    try {
      streamRef.removeQueuedPrompt(entry.queue, entry.id, entry.version)
      queuedUserMessages = queuedUserMessages.filter(message => message.id !== entry.id)
      openQueueSelector()
    } catch (err) {
      reconcileQueuedUserMessages()
      commitSystem('sys-queue-err', chalk.red(`  Queue remove failed: ${errorText(err)}`))
      openQueueSelector()
    }
  }

  /** Pull the newest queued prompt back into the editor without
   *  aborting the active run. Native optimistic version matching prevents an
   *  already-consumed prompt from being silently edited. */
  function restoreLastQueuedUserMessageToEditor() {
    if (!streamRef || queuedUserMessages.length === 0) return
    const queued = queuedUserMessages[queuedUserMessages.length - 1]!
    try {
      streamRef.removeQueuedPrompt(queued.queue, queued.id, queued.version)
      queuedUserMessages = queuedUserMessages.slice(0, -1)
      const next = mergeQueuedIntoEditorText([queued.text], getEditorText(editor))
      editor = insertText(clearEditor(editor), next)
      renderer.requestRender()
    } catch {
      // The engine already consumed it at a turn boundary; normal event handling
      // will commit the visible copy to history.
    }
  }

  /** Move mid-stream queued messages into the input box after an interrupt. */
  function restoreQueuedUserMessagesToEditor() {
    if (queuedUserMessages.length === 0) return
    const messages = queuedUserMessages.map(message => message.text)
    queuedUserMessages = []
    const next = mergeQueuedIntoEditorText(messages, getEditorText(editor))
    editor = insertText(clearEditor(editor), next)
    renderer.requestRender()
  }

  function handleLoadingEnter() {
    const displayText = getDisplayText()
    const historyText = getHistoryText()
    const imageResult = buildImageContentBlocks()
    const imageBlocks = imageResult?.blocks ?? null
    const expandedText = imageResult
      ? getExpandedText(imageResult.resolvedIds)
      : getExpandedText()

    const action = busySubmissionAction({
      displayText, expandedText, hasImages: imageBlocks !== null,
      compacting: manualCompaction.active, editingQueue: editingQueuedPrompt !== null,
      hasRun: streamRef !== null,
    })
    if (manualCompaction.active) {
      if (action === 'blocked_compaction_command') {
        // Not silently swallowed: commands cannot run mid-compaction and are
        // not queueable prompts. Keep the draft in the editor for later.
        commitSystem('sys-compact-cmd', "  Commands don't run during compaction. Press Esc to cancel it, or wait for it to finish.")
        renderer.requestRender()
        return
      }
      if (action === 'queue_compaction') {
        const queuedDisplay = displayText || '(image prompt)'
        queuedCompactionSubmissions.push({
          displayText: queuedDisplay,
          expandedText,
          ...(imageBlocks ? { contentJson: JSON.stringify(imageBlocks) } : {}),
        })
        if (historyText) {
          saveInputHistory(historyText)
        }
        clearAll()
        renderer.requestRender()
      }
      return
    }
    if (action === 'edit_queue') {
      saveQueueEdit(expandedText)
      return
    }
    if (action === 'show_log') {
      clearAll()
      const logPath = screenLog.filePath
      if (logPath) {
        const text = formatLogPaths(logPath, rendererTrace.filePath)
        commitSystem('sys-log', text ?? `  Log: ${logPath}`)
      }
      else commitSystem('sys-log', '  No active screen log.')
      renderer.requestRender()
      return
    }

    // Slash commands are not model input: queueing one as follow-up text
    // would send "/compact" to the LLM as conversation. Keep the draft in the
    // editor and tell the user instead.
    if (action === 'blocked_run_command') {
      commitSystem('sys-cmd-busy', "  Commands don't queue while a response is running. Press Esc to interrupt, or wait for the turn to finish.")
      renderer.requestRender()
      return
    }

    if (action === 'steer' && streamRef) {
      if (imageBlocks) {
        const contentJson = JSON.stringify(imageBlocks)
        const queued = streamRef.steer('', contentJson)
        queuedUserMessages.push({ ...queued, text: displayText || '(image prompt)', queue: 'steering' })
      } else {
        const queued = streamRef.steer(expandedText)
        if (displayText) queuedUserMessages.push({ ...queued, text: displayText, queue: 'steering' })
      }
      // Steering is only inspected between tool calls, so anything holding the
      // turn holds this message with it — a shell watched in the foreground or a
      // blocking task_output call alike. During that wait typing appears to do
      // nothing. Freeing both lets the message land now; the work keeps running.
      backgroundTerminals.reclaimTurnForMessage()
      // Save expanded text to input history before clearAll() drops the
      // in-memory paste registry. Keep displayText only for compact rendering.
      if (historyText) {
        saveInputHistory(historyText)
      }
      // Queue instead of committing now: history renders above the streaming
      // block, so an immediate commit lands above the incoming reply.
      // Plain and structured prompts are steering messages: consume them FIFO
      // at the next safe turn/tool boundary instead of waiting for the current
      // agent task to finish naturally.
      clearAll()
      renderer.requestRender()
    }
  }

  /** Commit queued prompts that are no longer present in either native queue. */
  function reconcileQueuedUserMessages() {
    if (queuedUserMessages.length === 0 || !streamRef) return
    let reconciliation: ReturnType<typeof reconcilePromptQueue>
    try {
      reconciliation = reconcilePromptQueue(readPromptQueues(streamRef), queuedUserMessages)
    } catch {
      return
    }
    const { ids: remainingIds, remaining, consumed } = reconciliation
    for (const message of consumed) commitLines(buildUserMessage(message.text))
    queuedUserMessages = remaining
    if (remaining.length === 0 && overlay.kind === 'selector' && overlay.state.owner === SELECTOR_OWNER.queue) {
      overlay = { kind: 'none' }
    }
    if (editingQueuedPrompt && !remainingIds.has(editingQueuedPrompt.id)) {
      finishQueueEdit()
      commitSystem('sys-queue-edit-consumed', '  Queued prompt was already consumed; edit closed.')
    }
  }

  function refreshFileCompletions(acceptSingle: boolean): void {
    const beforeCursor = editor.lines[editor.cursorLine]?.slice(0, editor.cursorCol) ?? ''
    if (!extractAtPrefix(beforeCursor)) return
    void fileCompletion.refresh(editor, appState.cwd, acceptSingle, () => editor, next => {
      editor = next
      renderer.requestRender()
    })
  }

  function deleteAtCursor() {
    const line = editor.lines[editor.cursorLine]!
    const deletedRef = parsePasteRefs(line).find(ref => ref.start === editor.cursorCol)
    if (deletedRef) {
      pastedChunks.delete(deletedRef.id)
      pastedImages.delete(deletedRef.id)
    }
    mutateEditor(state => deleteForward(state))
    renderer.requestRender()
  }

  function deleteWordAtCursor() {
    const lineIndex = editor.cursorLine
    const cursorCol = editor.cursorCol
    const refs = parsePasteRefs(editor.lines[lineIndex]!)
    mutateEditor(state => deleteWordBefore(state))
    if (editor.cursorLine === lineIndex) {
      for (const ref of refs) {
        if (ref.start < cursorCol && ref.end > editor.cursorCol) {
          pastedChunks.delete(ref.id)
          pastedImages.delete(ref.id)
        }
      }
    }
    renderer.requestRender()
  }

  function deleteWordForwardAtCursor() {
    const lineIndex = editor.cursorLine
    const before = editor.lines[lineIndex]!
    const refs = parsePasteRefs(before)
    mutateEditor(state => deleteWordForward(state))
    const after = editor.lines[editor.cursorLine] ?? ''
    for (const ref of refs) {
      if (!after.includes(ref.match)) {
        pastedChunks.delete(ref.id)
        pastedImages.delete(ref.id)
      }
    }
    renderer.requestRender()
  }

  function handleNormalKey(event: KeyEvent) {
    if (editor.completion && editor.completion.items.length > 0) {
      if (event.type === 'up' || event.type === 'down') {
        editor = moveCompletion(editor, event.type === 'up' ? -1 : 1)
        renderer.requestRender()
        return
      }
      if (event.type === 'enter' || event.type === 'tab') {
        editor = acceptCompletion(editor)
        renderer.requestRender()
        return
      }
    }

    if (event.type === 'ctrl') {
      switch (event.key) {
        case 'u':
          mutateEditor(state => clearLineBefore(state))
          renderer.requestRender()
          return
        case 'k':
          mutateEditor(state => clearLineAfter(state))
          renderer.requestRender()
          return
        case 'd':
          if (isEditorEmpty(editor)) {
            exitAfterCleanup(0)
            return
          }
          deleteAtCursor()
          return
        case 'w':
          deleteWordAtCursor()
          return
        case 'a':
          editor = moveHome(editor)
          renderer.requestRender()
          return
        case 'e':
          editor = moveEnd(editor)
          renderer.requestRender()
          return
        case 'l':
          clearAll()
          renderer.requestRender()
          return
        case 'v':
          tryPasteImage()
          return
        case 'o':
          toggleExpanded()
          return
        case '-':
          if (undoEditor()) renderer.requestRender()
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
          mutateEditor(state => insertContinuationNewline(state))
          renderer.requestRender()
          return
        }
        const displayText = getDisplayText()
        const historyText = getHistoryText()
        const imageResult = buildImageContentBlocks()
        const imageBlocks = imageResult?.blocks ?? null
        // expandedText: only strip image refs that have resolved data.
        // Unresolved ones (e.g. from history) stay as [Image #N] text markers.
        const expandedText = imageResult
          ? getExpandedText(imageResult.resolvedIds)
          : getExpandedText()
        // Allow image-only or text-only submissions
        if (!expandedText && !imageBlocks) return
        if (historyText) saveInputHistory(historyText)
        clearAll()
        renderer.requestRender()
        if (isSlashCommand(expandedText || rawText)) {
          handleSlashInput(expandedText || rawText)
        } else if (logMode) {
          // In log mode, send to forked agent
          runLogQuery(logMode, expandedText)
        } else {
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
      case 'shift-enter':
      case 'ctrl-enter':
      case 'alt-enter': {
        mutateEditor(state => insertNewline(state))
        renderer.requestRender()
        break
      }
      case 'shift-tab': {
        cycleThinkingLevel()
        break
      }
      case 'tab': {
        const beforeCursor = editor.lines[editor.cursorLine]!.slice(0, editor.cursorCol)
        if (extractAtPrefix(beforeCursor)) {
          refreshFileCompletions(true)
          break
        }
        const previous = editor
        const result = applyCompletion(editor)
        if (result.applied) {
          if (getEditorText(result.state) !== getEditorText(previous)) {
            editorUndo.push(previous)
          }
          editor = result.state
          renderer.requestRender()
        }
        break
      }
      case 'char':
      case 'shift-char':
        mutateEditor(state => refreshGhostHint(insertText(state, event.char)))
        renderer.requestRender()
        if (extractAtPrefix(editor.lines[editor.cursorLine]!.slice(0, editor.cursorCol))) {
          refreshFileCompletions(false)
        }
        break
      case 'paste':
        insertPaste(event.text)
        renderer.requestRender()
        break
      case 'paste-clipboard':
        void tryPasteClipboard()
        break
      case 'delete':
        deleteAtCursor()
        break
      case 'backspace': {
        const currentLine = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(currentLine)
        const refDel = deleteRefBackspace(currentLine, editor.cursorCol, refs)
        if (refDel) {
          const deletedRef = refs.find(ref => ref.end === editor.cursorCol)
          if (deletedRef) {
            pastedChunks.delete(deletedRef.id)
            pastedImages.delete(deletedRef.id)
          }
          mutateEditor(state => ({
            ...state,
            lines: state.lines.map((line, index) =>
              index === state.cursorLine ? refDel.newLine : line),
            cursorCol: refDel.newCursorCol,
            preferredVisualCol: undefined,
            ghostHint: '',
            completion: null,
          }))
        } else {
          mutateEditor(state => backspace(state))
        }
        editor = refreshGhostHint(editor)
        renderer.requestRender()
        if (extractAtPrefix(editor.lines[editor.cursorLine]!.slice(0, editor.cursorCol))) {
          refreshFileCompletions(false)
        }
        break
      }
      case 'word-left':
        editor = moveWordLeft(editor)
        renderer.requestRender()
        break
      case 'word-right':
        editor = moveWordRight(editor)
        renderer.requestRender()
        break
      case 'alt-backspace':
        deleteWordAtCursor()
        break
      case 'alt-d':
        deleteWordForwardAtCursor()
        break
      case 'undo':
        if (undoEditor()) renderer.requestRender()
        break
      case 'left':
        editor = moveLeft(editor)
        renderer.requestRender()
        break
      case 'right':
        editor = moveRight(editor)
        renderer.requestRender()
        break
      case 'home':
        editor = moveHome(editor)
        renderer.requestRender()
        break
      case 'end':
        editor = moveEnd(editor)
        renderer.requestRender()
        break
      case 'up': {
        const moved = moveUp(editor, Math.max(1, renderer.termCols - 2))
        if (moved !== editor) {
          editor = moved
          renderer.requestRender()
          break
        }
        // At the top visual row: navigate history.
        const result = historyPrev(historyState, editor)
        if (result.changed) {
          historyState = result.history
          inputImageHistory.capture(getEditorText(editor), pastedImages)
          editor = result.editor
          inputImageHistory.restore(getEditorText(editor), pastedImages)
          renderer.requestRender()
        }
        break
      }
      case 'down': {
        const moved = moveDown(editor, Math.max(1, renderer.termCols - 2))
        if (moved !== editor) {
          editor = moved
          renderer.requestRender()
          break
        }
        // At the bottom visual row: navigate history.
        const result = historyNext(historyState, editor)
        if (result.changed) {
          historyState = result.history
          inputImageHistory.capture(getEditorText(editor), pastedImages)
          editor = result.editor
          inputImageHistory.restore(getEditorText(editor), pastedImages)
          renderer.requestRender()
          break
        }
        // Nothing left for ↓ to do here, so it opens the background panel the
        // prompt hint is advertising. Cursor movement and history both had
        // their chance first, so no existing gesture is taken away.
        backgroundTerminals.handlePromptDown(isEditorEmpty(editor))
        break
      }
      case 'page-up':
      case 'page-down':
        break
      default:
        break
    }
  }

  async function handleSlashInput(text: string) {
    const pendingCommand = resolveCommand(text)
    if (pendingCommand.kind === 'resolved' && backgroundTerminals.guardSessionSwitch(pendingCommand.name)) {
      return
    }
    // `/model` used to await a cloud catalog fetch here, which froze the TUI
    // for the whole HTTP round-trip. Open on the cached list instead; a
    // background sync below refreshes the overlay when the catalog lands.
    if (pendingCommand.kind === 'resolved' && pendingCommand.name === '/model') {
      try {
        configInfo = agent.configInfo()
      } catch (err) {
        commitSystem('sys-model-config', chalk.red(`  Failed to reload model config: ${errorText(err)}`))
        renderer.requestRender()
        return
      }
      void syncCloudNow(true)
    }

    let result
    try {
      result = handleSlashCommand(text, {
        agent,
        appState,
        configInfo,
        planning,
      })
    } catch (err) {
      commitSystem('sys-command-err', chalk.red(`  Command failed: ${errorText(err)}`))
      renderer.requestRender()
      return
    }
    appState = result.appState
    planning = result.planning
    if (result.overlay) overlay = result.overlay
    if (result.clearContext) {
      // Abort any in-flight streaming and clear local context view without switching sessions.
      if (isLoading && streamRef) {
        revokeRun()
        const interruptedStream = streamRef
        streamRef = null
        interruptedStream.abort()
        isLoading = false
        flushStreamContent()
        streamMachine = null
        stopSpinner()
      }
      sessionHook.endSession('context_cleared')
      sessionId = null
      planModeItems = []
      lastReviewedPlanMarkdown = ''
      appState = { ...createInitialState(appState.model, agent.cwd) }
      gitInfo.setCwd(agent.cwd)
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      resetHistoryCache()
      try { preloadedSessions = await agent.listSessions(20) } catch {}
      resumeCache.replace(preloadedSessions)
    }
    if (result.newSession) {
      // Abort any in-flight streaming.
      if (isLoading && streamRef) {
        revokeRun()
        const interruptedStream = streamRef
        streamRef = null
        interruptedStream.abort()
        isLoading = false
        flushStreamContent()
        streamMachine = null
        stopSpinner()
      }
      planModeItems = []
      lastReviewedPlanMarkdown = ''
      // Leave the session unbound until the first real prompt. The query path
      // creates and returns the session id, so abandoning /new no longer leaves
      // an empty, untitled session in persistent storage or /resume.
      sessionHook.endSession('new_session')
      sessionId = null
      appState = { ...createInitialState(appState.model, agent.cwd) }
      gitInfo.setCwd(agent.cwd)
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      resetHistoryCache()
      commitSystem('sys-new-session', chalk.dim('  new session'))
    }
    if (result.exit) { exitAfterCleanup(0); return }
    if (result.restart) { restartAfterCleanup(); return }
    if (result.systemLines.length > 0) commitSystemLines(result.systemLines)

    // Handle async commands that the simple handleSlashCommand can't do
    const resolved = resolveCommand(text)
    if (resolved.kind !== 'resolved') {
      renderer.requestRender()
      return
    }
    const { name, args } = resolved

    if (name === '/model' && args) {
      refreshConfigInfo()
      appState = { ...appState, model: agent.model }
    }

    if (name === '/plan') {
      planModeItems = []
      lastReviewedPlanMarkdown = ''
      renderer.requestRender()
    }

    if (name === '/compact') {
      await runManualCompaction(args)
    } else if (name === '/env') {
      await handleEnvCommand(replCommands, args)
    } else if (name === '/harden') {
      const subject = buildHardenPrompt(args)
      commitLines(buildUserMessage(text.trim()))
      runQuery(subject)
    } else if (name === '/clip') {
      const sub = args.trim().toLowerCase()
      if (sub === 'all') {
        // /clip all is expanded server-side into the memory skill's session
        // distillation prompt; bare /clip remains a zero-token local action.
        commitLines(buildUserMessage(text.trim()))
        runQuery('/clip all')
      } else if (sub) {
        commitSystem('sys-clip-err', '  Usage: /clip [all]')
      } else {
        await handleClipCommand(replCommands)
      }
    } else if (name === '/share') {
      await handleShareCommand(replCommands, args)
    } else if (name === '/skill') {
      try {
        await handleSkillCommand(replCommands, args)
      } finally {
        refreshBannerData()
      }
    } else if (name === '/copy') {
      await handleCopyCommand(replCommands)
    } else if (name === '/update') {
      await handleUpdateCommand(replCommands)
    } else if (name === '/version') {
      await handleVersionCommand(replCommands)
    } else if (name === '/login') {
      // Device-code flow in place so free models appear without restarting.
      // Wait for an admin-sign-out cleanup before checking local identity;
      // otherwise a fast /login can race the auth-file removal.
      if (loginInFlight) {
        commitSystem('sys-login', '  login already in progress')
      } else {
        // Wait for an in-flight recovery first: it decides whether this login is
        // even needed, and a fast /login could otherwise race the auth cleanup.
        if (revocationCleanup) await revocationCleanup
        const { authWhoami } = await import('../native/index.js')
        const { decideLoginGate } = await import('../commands/login-flow.js')
        // `cloudLoginRequired` is set only after the server refused the stored
        // CLI token, so a leftover auth.json can never block a fresh flow.
        const existing = cloudLoginRequired ? null : await authWhoami()
        const gate = decideLoginGate(existing, cloudLoginRequired)
        if (gate.kind === 'already-logged-in') {
          commitSystem('sys-login', `  already logged in as ${gate.user.name} <${gate.user.email}>`)
        } else {
          loginInFlight = true
          try {
            const loggedIn = await handleLoginCommand(replCommands)
            if (loggedIn) {
              try {
                cloudLoginRequired = !reloadAfterAuthChange()
                authWatcher?.sync()
                if (cloudLoginRequired) {
                  commitSystem('sys-login-model-err', chalk.red('  Login succeeded, but no cloud model was loaded. Try /login again.'))
                } else {
                  void syncCloudNow(true)
                }
              } catch (err) {
                cloudLoginRequired = true
                commitSystem('sys-login-model-err', chalk.red(`  Failed to load the signed-in cloud model: ${errorText(err)}`))
              }
            }
          } finally {
            loginInFlight = false
          }
        }
      }
    } else if (name === '/logout') {
      if (loginInFlight) {
        commitSystem('sys-logout', '  login in progress — wait or restart to log out')
      } else {
        if (revocationCleanup) await revocationCleanup
        const loggedOut = await handleLogoutCommand(replCommands)
        if (loggedOut) {
          try {
            cloudLoginRequired = !reloadAfterAuthChange()
            authWatcher?.sync()
          } catch (err) {
            cloudLoginRequired = true
            commitSystem('sys-logout-model-err', chalk.red(`  Failed to reload providers after logout: ${errorText(err)}`))
          }
        }
      }
    } else if (name === '/act' || name === '/done') {
      if (logMode) {
        logMode = null
        commitSystem('sys-log-exit', '  [log mode] exited')
      } else {
        planning = false
        planModeItems = []
        commitSystem('sys-act', '  planning: off')
      }
    } else if (name === '/_dump') {
      try {
        const outcome = await agent.submit(
          `/_dump${args ? ' ' + args : ''}`,
          sessionId ?? undefined,
          planning ? 'planning_interactive' : 'interactive',
        )
        if (outcome.kind === 'command') {
          const lines = (outcome.message ?? '').split('\n').map((line, i) => ({
            id: `sys-dump-${i}`,
            kind: 'system' as const,
            text: `  ${line}`,
          }))
          commitLines(lines.length > 0 ? lines : [{ id: 'sys-dump', kind: 'system', text: '  (no dump output)' }])
        }
      } catch (err) {
        commitSystem('sys-dump-err', chalk.red(`  /_dump failed: ${errorText(err)}`))
      }
    } else if (name === '/log') {
      await handleLogCommand(args)
    } else if (name === '/resume') {
      const query = normalizeResumeQuery(args)
      try {
        if (query && isSessionIdPrefix(query)) {
          const allSessions = await resumeCache.all()
          const resolved = resolveSessionByPrefix(allSessions, query)
          if (resolved.kind === 'matched') {
            await resumeSession(resolved.session)
          } else {
            openResumeSelector(query)
          }
        } else if (query) {
          await handleSemanticResume(query)
        } else {
          openResumeSelector(undefined)
        }
      } catch (err) {
        commitSystem('sys-r-err', chalk.red(`  Failed to list sessions: ${errorText(err)}`))
      }
    } else if (name === '/model' && !args) {
      openModelSelector()
    }

    renderer.requestRender()
  }

  /**
   * Semantic session search for `/resume <query>`: the server ranks recent
   * sessions with a one-shot LLM call (hidden `/_rsearch` command). Shows the
   * ranked list with reasons, then opens the resume selector on the ranked
   * sessions. Falls back to the literal-filter selector on failure.
   */
  function cloudCampaigns(notices = authNotices()) {
    return campaignContent(notices)
  }

  /** Re-read the synced catalog: refresh model config and the ad/notice slot.
   *  Called after login, by the live-sync poller, and on demand. */
  function reloadCloudContent(fresh = cloudCampaigns()): void {
    refreshCampaigns(adSlot, fresh, hasPremiumModel(configInfo), Date.now())
    try { configInfo = agent.configInfo() } catch {}
  }

  // Live sync: re-fetch the cloud catalog so new notices, ads and models
  // appear in-session without a restart. Silent when nothing changed.
  let inflightSync: Promise<void> | null = null
  const CLOUD_SYNC_MS = 15_000
  const cloudSync = new CloudSync({
    authenticated: async () => {
      const { authWhoami } = await import('../native/index.js')
      return Boolean(await authWhoami())
    },
    syncNotices: async () => {
      const { authSyncNotices } = await import('../native/index.js')
      return authSyncNotices()
    },
    syncModels: async () => {
      const { authSyncModels } = await import('../native/index.js')
      return authSyncModels()
    },
    noticesUpdated: notices => reloadCloudContent(cloudCampaigns(notices)),
    modelsUpdated: () => {
      authWatcher?.sync()
      reloadCloudContent()
    },
  }, CLOUD_SYNC_MS)
  resources.add(() => cloudSync.dispose())
  const syncTimer = setInterval(() => {
    void syncCloudNow()
  }, CLOUD_SYNC_MS)
  ;(syncTimer as unknown as { unref?: () => void }).unref?.()
  resources.add(() => clearInterval(syncTimer))
  const backgroundProcessTimer = setInterval(refreshBackgroundProcesses, 500)
  ;(backgroundProcessTimer as unknown as { unref?: () => void }).unref?.()
  resources.add(() => clearInterval(backgroundProcessTimer))

  /** Adopt a cloud auth change made by another evot process. */
  function adoptExternalAuthChange(): void {
    // A recovery in flight owns the auth state and syncs the stamp itself.
    if (revocationCleanup) return
    try {
      const reloaded = reloadAfterAuthChange()
      cloudLoginRequired = !reloaded && !configInfo?.hasApiKey
      if (cloudLoginRequired) {
        commitCloudSession('  ⚠ Cloud session signed out · run /login to reconnect', 'warn')
      } else {
        renderer.requestRender()
      }
    } catch {
      // Half-written file from a concurrent login; the next event retries.
    }
  }

  authWatcher = new AuthWatcher(() => adoptExternalAuthChange())

  async function syncCloudNow(force = false): Promise<void> {
    if (destroyed) return
    if (inflightSync) return inflightSync
    inflightSync = (async () => {
      // Any server-pushed group counts, not just the free tier, so granted
      // models and split protocol groups are announced too.
      const cloudModels = () => configInfo?.availableModels.filter(isCloudModel) ?? []
      const knownModelIds = new Set(cloudModels().map(m => m.model))
      const knownCampaignIds = new Set([...adSlot.notices, ...adSlot.ads].map(c => c.id))
      const knownFingerprints = new Set([...adSlot.notices, ...adSlot.ads].map(campaignFingerprint))

      const synced = await cloudSync.run(force)
      if (destroyed || !synced) return
      const { noticesSynced, modelsSynced } = synced
      if (!noticesSynced && !modelsSynced) return

      const addedModels = cloudModels().filter(m => !knownModelIds.has(m.model))
      const campaigns = [...adSlot.notices, ...adSlot.ads]
      const addedCampaigns = campaigns.filter(c => !knownCampaignIds.has(c.id))
      const copyChanged = campaigns.some(c => !knownFingerprints.has(campaignFingerprint(c)))
      if (addedModels.length > 0) {
        const names = addedModels.slice(0, 3)
          .map(m => formatModelOptionLabel(m))
          .join(', ')
        const more = addedModels.length > 3 ? ` and ${addedModels.length - 3} more` : ''
        const notice = `  ✓ New models available: ${names}${more} — /model to switch`
        if (commandWindowMounted()) deferredCloudModelNotice = notice
        else commitSystem('sys-cloud-models', chalk.dim(notice))
      }
      if (addedCampaigns.length > 0) {
        // A command window owns stable geometry. Defer the idle campaign
        // transition until it closes rather than inserting rows above it.
        const campaignId = addedCampaigns[0]!.id
        if (commandWindowMounted()) deferredCloudCampaignId = campaignId
        else showCloudCampaign(campaignId)
      }
      const refreshedOpenPicker = modelsSynced && refreshOpenModelSelector()
      if (
        refreshedOpenPicker
        || (!commandWindowMounted() && (addedModels.length > 0 || addedCampaigns.length > 0 || copyChanged))
      ) {
        renderer.requestRender()
      }
    })()
    try {
      await inflightSync
    } finally {
      inflightSync = null
    }
  }

  // First pull as soon as the prompt is up, so ads/models aren't 30s stale.
  const initialSyncTimer = setTimeout(() => { void syncCloudNow() }, 400)
  initialSyncTimer.unref?.()
  resources.add(() => clearTimeout(initialSyncTimer))

  function openModelSelector(): void {
    overlay = { kind: 'selector', state: createModelWindow(configInfo, agent.model, true) }
  }

  /** Swap the open or previewed /model list in place after a catalog refresh.
   *  Keeps the current query and focused row so typing isn't yanked around. */
  function refreshOpenModelSelector(): boolean {
    const models = modelOptions(configInfo, agent.model)
    const activeSpec = currentModelSpec(configInfo, agent.model)
    if (overlay.kind === 'selector' && overlay.state.owner === SELECTOR_OWNER.model) {
      overlay = {
        kind: 'selector',
        state: selectorExpandItems(overlay.state, modelSelectorItems(models, activeSpec)),
      }
      return true
    }
    if (commandWindowPreview?.kind === 'selector' && commandWindowPreview.trigger === 'model') {
      commandWindowPreview = {
        ...commandWindowPreview,
        state: selectorExpandItems(commandWindowPreview.state, modelSelectorItems(models, activeSpec)),
      }
      return true
    }
    return false
  }

  async function handleSemanticResume(query: string) {
    commitSystem('sys-rsem-busy', chalk.dim('  searching sessions…'))
    renderer.requestRender()
    try {
      const outcome = await agent.submit(`/_rsearch ${query}`, sessionId ?? undefined, 'interactive')
      if (outcome.kind !== 'command') return
      const message = outcome.message ?? ''
      const lines = message.split('\n')
      commitLines(lines.map((line, i) => ({
        id: `sys-rsem-${i}`,
        kind: 'system' as const,
        text: `  ${line}`,
      })))
      const ids: string[] = []
      for (const line of lines) {
        const m = /^- (\S+) — /.exec(line)
        if (m) ids.push(m[1]!)
      }
      if (ids.length === 0) return
      const allSessions = await resumeCache.all()
      const ranked = ids
        .map(id => allSessions.find(s => s.session_id === id))
        .filter((s): s is SessionMeta => Boolean(s))
      if (ranked.length === 0) return
      const items = formatSessionItems(ranked, agent.cwd, id => resumeCache.sessionText(id))
      invalidateExplicitResumeSelector()
      overlay = {
        kind: 'selector',
        state: resumeSelectorState(items),
      }
      renderer.requestRender()
    } catch (err) {
      commitSystem('sys-rsem-err', chalk.red(`  Semantic search failed: ${errorText(err)}`))
      openResumeSelector(query)
    }
  }

  function openResumeSelector(initialQuery?: string) {
    const generation = ++explicitResumeSelectorGeneration
    const cached = resumeCache.withText ?? resumeCache.metadata
    const items = cached === null
      ? []
      : formatSessionItems(cached, agent.cwd, id => resumeCache.sessionText(id))
    overlay = {
      kind: 'selector',
      state: {
        ...resumeSelectorState(items, initialQuery),
        ...(cached === null ? { emptyMessage: 'Loading sessions…' } : {}),
      },
    }
    renderer.requestRender()
    if (cached !== null) loadFocusedResumePreview(overlay.state)

    const activeState = (): SelectorState | null => {
      if (generation !== explicitResumeSelectorGeneration) return null
      if (overlay.kind !== 'selector' || overlay.state.owner !== SELECTOR_OWNER.resume) return null
      return overlay.state
    }

    resumeCache.all().then(allSessions => {
      const current = activeState()
      if (!current) return
      if (allSessions.length === 0) {
        invalidateExplicitResumeSelector()
        overlay = { kind: 'none' }
        commitSystem('sys-r', '  No sessions found')
        renderer.requestRender()
        return
      }
      const metaItems = formatSessionItems(allSessions, agent.cwd, id => resumeCache.sessionText(id))
      overlay = {
        kind: 'selector',
        state: selectorExpandItems(current, metaItems),
      }
      renderer.requestRender()
      loadFocusedResumePreview(overlay.state)
    }).catch((err: unknown) => {
      if (!activeState()) return
      commitSystem('sys-r-err', chalk.red(`  Failed to list sessions: ${errorText(err)}`))
    })
  }

  async function handleLogCommand(args: string) {
    const query = args.trim()
    const { join } = await import('path')
    const { homedir } = await import('os')
    const logDir = join(homedir(), '.evotai', 'logs')
    const sid = sessionId

    if (query === 'shot' || query.startsWith('shot ')) {
      // /log shot exports the latest committed assistant turn from in-memory
      // history. This keeps renderer diagnostics off the TUI hot path.
      const unsupportedTarget = query.slice(4).trim()
      if (unsupportedTarget) {
        commitSystem('sys-log-shot-target', '  /log shot exports the latest assistant turn; message ids are no longer supported.')
        return
      }
      foregroundCommand = 'log-shot'
      isLoading = true
      spinnerState = setSpinnerPhase(createSpinnerState(), 'executing', 'log_shot_render')
      startSpinner()
      renderer.requestRender()
      // Let the status row paint before markdown rendering starts synchronously.
      await Bun.sleep(0)
      try {
        const { writeMarkdownShot } = await import('../commands/log-shot.js')
        const result = await writeMarkdownShot({
          historyLines: compactLines,
          columns: renderer.termCols,
          open: false,
          onProgress: stage => {
            const toolName = stage === 'starting_chrome'
              ? 'log_shot_chrome'
              : stage === 'capturing_png'
                ? 'log_shot_capture'
                : stage === 'opening_html'
                  ? 'log_shot_open'
                  : 'log_shot_render'
            spinnerState = setSpinnerPhase(spinnerState, 'executing', toolName)
            renderer.requestRender()
          },
          header: {
            model: appState.model || agent.model,
            thinkingLevel: configInfo?.thinkingLevel,
            sessionId: sessionId ?? undefined,
            cwd: agent.cwd,
            branch: gitInfo.getBranch() ?? undefined,
          },
        })
        const lines = [
          `  Shot: ${result.messageId}${result.chunkCount > 1 ? ` (${result.chunkCount} chunks)` : ''}`,
          `  HTML: ${result.htmlPath}`,
        ]
        if (result.pngPath) lines.push(`  PNG:  ${result.pngPath}`)
        else lines.push('  PNG:  (no Chrome/Chromium — HTML only. Install chromium or set EVOT_CHROME.)')
        commitSystem('sys-log-shot', lines.join('\n'))
      } catch (err) {
        commitSystem('sys-log-err', chalk.red(`  Shot failed: ${errorText(err)}`))
      } finally {
        foregroundCommand = null
        isLoading = false
        stopSpinner()
        renderer.requestRender()
      }
    } else if (!query) {
      const logPath = screenLog.filePath
      if (logPath) {
        const text = formatLogPaths(logPath, rendererTrace.filePath)
        commitSystem('sys-log', text ?? `  Log: ${logPath}`)
      }
      else if (sid) {
        const text = formatLogPaths(join(logDir, `${sid}.screen.log`))
        commitSystem('sys-log', text ?? `  Log: ${join(logDir, `${sid}.screen.log`)}`)
      }
      else commitSystem('sys-log', `  Log dir: ${logDir} (no active session)`)
    } else if (!sid) {
      commitSystem('sys-log-err', '  No active session to analyze.')
    } else {
      // /log <query> — fork agent to analyze log
      const logPath = join(logDir, `${sid}.screen.log`)
      const systemPrompt = [
        'You are in a temporary log analysis session.',
        'This session is not persisted and does not affect the main session context.',
        '',
        `Screen log file to analyze:\n${logPath}`,
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
        commitSystem('sys-log-mode', `  [log mode] analyzing: ${logPath}\n  not persisted. press Esc to exit.`)
        renderer.requestRender()
        await runLogQuery(forked, query)
      } catch (err) {
        commitSystem('sys-log-err', chalk.red(`  Fork failed: ${errorText(err)}`))
      }
    }
    renderer.requestRender()
  }

  async function runLogQuery(forked: import('../native/index.js').ForkedAgent, prompt: string) {
    if (queryBlockedByCloudLogin()) return
    const generation = beginRun()
    liveContentMaxHeight = 0
    isLoading = true
    spinnerState = createSpinnerState()
    streamMachine = createStreamMachineState(appState, spinnerState)
    startSpinner()
    renderer.requestRender()
    commitLines(buildUserMessage(prompt))

    try {
      const stream = await forked.query(prompt)
      if (!ownsRun(generation)) {
        stream.abort()
        return
      }
      streamRef = stream

      // Reuse the main streaming path so log-mode inherits pi-aligned behavior:
      // the whole message stays in the dynamic zone and only commits at
      // markdown-safe boundaries, so tables/lists/code blocks are never torn
      // (the old per-newline commit here split every table row into its own
      // buildAssistantLines call).
      for await (const event of stream) {
        if (destroyed || !ownsRun(generation)) break
        if (!streamMachine) break
        if (isHostToolEvent(event)) throw new Error('Readonly log run requested a host tool')

        const update = reduceRunEvent(streamMachine, event, {
          termRows: renderer.termRows,
          cloudProvider: activeProviderIsCloud(),
        })
        streamMachine = update.state
        appState = update.state.appState
        spinnerState = update.state.spinnerState
        if (update.sessionRevoked) handleCloudSessionRevoked()

        if (event.kind === 'assistant_delta') renderer.requestRender()
        if (update.commitLines.length > 0) commitLines(update.commitLines)
        if (event.kind === 'turn_started') reconcileQueuedUserMessages()
        if (update.rerenderStatus) renderer.requestRender()
      }

      if (!ownsRun(generation)) return
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        appState = final.state.appState
        commitFlushResult(final)
      }
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
    } catch (err) {
      if (!ownsRun(generation)) return
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        commitFlushResult(final)
      }
      commitSystem('sys-log-err', chalk.red(`  Log query failed: ${errorText(err)}`))
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
    } finally {
      if (ownsRun(generation)) {
        streamRef = null
        isLoading = false
        streamMachine = null
        stopSpinner()
        renderer.requestRender()
      }
    }
  }

  function handleSelectorKey(event: KeyEvent) {
    if (overlay.kind !== 'selector') return
    const action = handleSelectorControl(overlay.state, event)

    switch (action.kind) {
      case 'update':
        overlay = { kind: 'selector', state: action.state }
        renderer.requestRender()
        if (action.state.owner === SELECTOR_OWNER.resume) {
          if (focusedCommandWindowGeneration !== null) {
            scheduleFocusedResumeEnrichment(
              focusedCommandWindowGeneration,
              action.state.query.length > 0,
            )
          }
          loadFocusedResumePreview(action.state)
        }
        return
      case 'close':
        invalidateExplicitResumeSelector()
        overlay = { kind: 'none' }
        focusedCommandWindowGeneration = null
        cancelResumeSearchEnrichment()
        nextCommandWindowGeneration()
        releaseCommandWindowLayout()
        renderer.requestRender()
        return
      case 'resume':
        invalidateExplicitResumeSelector()
        overlay = { kind: 'none' }
        focusedCommandWindowGeneration = null
        nextCommandWindowGeneration()
        clearAll()
        releaseCommandWindowLayout()
        resumeSession({ session_id: action.sessionId } as SessionMeta).then(() => renderer.requestRender())
        renderer.requestRender()
        return
      case 'select-model': {
        overlay = { kind: 'none' }
        focusedCommandWindowGeneration = null
        nextCommandWindowGeneration()
        clearAll()
        releaseCommandWindowLayout()
        try {
          agent.setProvider(action.spec)
          refreshConfigInfo()
          const selected = selectModelOption(configInfo, action.spec)
          const model = selected?.model ?? agent.model
          const provider = selected?.provider ?? configInfo?.provider ?? ''
          appState = { ...appState, model }
          commitStatusLine({ id: 'sys-model', kind: 'system', text: `  Model → ${formatModelLabel(model, provider, selected?.group_label)}` })
        } catch (err) {
          commitSystem('sys-model-err', chalk.red(`  Failed to switch model: ${errorText(err)}`))
        }
        renderer.requestRender()
        return
      }
      case 'delete-session':
        overlay = { kind: 'selector', state: action.state }
        agent.deleteSession(action.sessionId).then(ok => {
          if (ok) {
            preloadedSessions = preloadedSessions.filter(session => session.session_id !== action.sessionId)
            resumeCache.remove(action.sessionId, preloadedSessions)
            commitSystem('sys-del', `  Deleted session ${action.label}`)
            // Also surface it on the overlay: resuming another session clears
            // the screen, which would otherwise wipe the only confirmation.
            if (overlay.kind === 'selector' && overlay.state.owner === SELECTOR_OWNER.resume) {
              overlay = {
                kind: 'selector',
                state: { ...overlay.state, subtitle: `Deleted ${action.label}` },
              }
              renderer.requestRender()
            }
          }
        })
        renderer.requestRender()
        return
      case 'queue-edit':
        editQueuedPrompt(action.entry)
        return
      case 'queue-remove':
        removeQueuedPrompt(action.entry)
        return
      case 'none':
        return
    }
  }

  function handleAskKey(event: KeyEvent) {
    if (overlay.kind !== 'ask-user') return

    const extra = event.type === 'char'
      ? event.char
      : event.type === 'paste'
        ? event.text
        : event.type === 'ctrl' && (event.key === 'n' || event.key === 'p')
          ? event.key
          : undefined
    const eventType = event.type === 'ctrl' && (event.key === 'n' || event.key === 'p')
      ? `ctrl+${event.key}`
      : event.type
    const result = handleAskKeyEvent(overlay.state, eventType, extra)

    switch (result.action) {
      case 'cancel':
        // Resolve the awaiting host tool as cancelled; the tool maps this to a
        // tool error so the engine's run continues rather than hanging.
        resolvePendingAsk()
        overlay = { kind: 'none' }
        unfreezeTerminalTitle()
        commitSystem('sys-ask-cancel', '  ⏺ Cancelled.')
        renderer.requestRender()
        return
      case 'submit':
        {
          const response = askStateToResponse(result.state)
          if (pendingAsk) {
            pendingAsk(response)
            pendingAsk = null
          }
          overlay = { kind: 'none' }
          unfreezeTerminalTitle()
          const answerLines: OutputLine[] = response.flatMap((r, i) => ([
            {
              id: `sys-ask-${i}-question`,
              kind: 'system' as const,
              text: `  • ${r.question}`,
            },
            {
              id: `sys-ask-${i}-answer`,
              kind: 'system' as const,
              text: `    → ${r.answer}`,
            },
          ]))
          commitLines(answerLines)
        }
        renderer.requestRender()
        return
      case 'update':
        overlay = { kind: 'ask-user', state: result.state }
        renderer.requestRender()
        return
    }
  }

  function applyDetectedTheme(scheme: 'dark' | 'light'): void {
    // Committed history is pre-painted ANSI; a scheme change must rebuild it.
    if (!setDetectedThemeScheme(scheme)) return
    resetHistoryCache()
    partialBlocksMemo = null
    renderer.requestRender(true)
  }

  function handleTerminalControl(event: TerminalControlEvent): void {
    if (event.type === 'osc11-background') {
      applyDetectedTheme(schemeFromRgbColor(event.rgb))
      return
    }
    if (event.type === 'color-scheme') {
      applyDetectedTheme(event.scheme)
      return
    }
    enhancedKeyboard?.handleControl(event)
  }

  disableRaw = enableRawMode(process.stdin)
  const terminalInput = new TerminalInputBuffer({
    onEmptyPaste: tryPasteImage,
    onControl: handleTerminalControl,
  })
  inputBuffer = terminalInput
  const dispatchInputEvents = (events: KeyEvent[]) => {
    for (const event of events) {
      if (destroyed) break
      handleKey(event)
    }
  }
  const inputHandler = (data: Buffer | string) => {
    if (escapeFlushTimer) {
      clearTimeout(escapeFlushTimer)
      escapeFlushTimer = undefined
    }
    dispatchInputEvents(terminalInput.write(data))
    if (terminalInput.hasAmbiguousEscape) {
      escapeFlushTimer = setTimeout(() => {
        escapeFlushTimer = undefined
        dispatchInputEvents(terminalInput.flushPending())
      }, 10)
    }
  }
  onInputData = inputHandler
  process.stdin.on('data', inputHandler)
  enhancedKeyboard = enableEnhancedKeyboard(process.stdout)
  // Query terminal theme (OSC 11 + color-scheme DSR) and subscribe to mode 2031
  // palette-change notifications. Unsupported terminals simply never reply.
  process.stdout.write('\x1b]11;?\x07\x1b[?996n\x1b[?2031h')

  process.stdout.write('\x1b[?2004h')
  renderer.requestRender()

  function cleanup() {
    if (destroyed) return
    destroyed = true
    queuedCompactionSubmissions = []
    unfreezeTerminalTitle()
    manualCompaction.abort()
    streamRef?.abort()
    stopSpinner()
    adSlotWakeup.dispose()
    resources.dispose()
    authWatcher?.dispose()
    authWatcher = null
    caretBlink.dispose()
    gitInfo.dispose()
    updateMgr.cleanup()
    if (exitHintTimer) clearTimeout(exitHintTimer)
    cancelResumeCommandLoad()
    cancelResumeSearchEnrichment()
    invalidateExplicitResumeSelector()
    cancelSearchTextWarm()
    committer.flushReveals()
    if (escapeFlushTimer) {
      clearTimeout(escapeFlushTimer)
      escapeFlushTimer = undefined
    }
    if (onInputData) {
      process.stdin.off('data', onInputData)
      onInputData = null
    }
    inputBuffer?.discard()
    inputBuffer = null
    process.stdout.write('\x1b[?2004l')
    process.stdout.write('\x1b[?2031l')
    enhancedKeyboard?.dispose()
    enhancedKeyboard = null
    setTerminalTitle()
    disableRaw?.()
    disableRaw = null
    renderer.destroy()
    rendererTrace.close()
    // Every exit path ends in fastExit, which skips Rust Drop impls and async
    // teardown. Background children live in their own process groups, so they
    // would survive as orphans unless signalled synchronously right here.
    // The controller absorbs its own failures: cleanup must never throw.
    backgroundCleanup.dispose()
  }

  function restartAfterCleanup(): void {
    cleanup()
    void sessionHook.close().then(async () => {
      const { execIntoInstalledRestart } = await import('../update/index.js')
      execIntoInstalledRestart(sessionId)
      // Handover only returns when execve was impossible (source checkout,
      // Windows, missing binary). Fall back to a normal exit so the user can
      // relaunch from the shell instead of sitting in a dead TUI.
      fastExit(0)
    })
  }

  // Declared as a function so the earlier exit paths (Ctrl-D, /exit, the exit
  // control action) can call it without hitting a `const` TDZ error: they run
  // from callbacks defined above this point in `startRepl`.
  function exitAfterCleanup(code: number): void {
    cleanup()
    void sessionHook.close().then(() => fastExit(code))
  }

  process.on('SIGINT', () => { exitAfterCleanup(130) })
  process.on('SIGTERM', () => { exitAfterCleanup(143) })

  await new Promise<void>(() => {})
}
