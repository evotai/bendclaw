/**
 * Background terminal management for the TUI.
 *
 * Owns the polled task list, the interactive panel (`ctrl+t` or `↓` on an empty
 * composer), and the session-switch gate. Every gesture lives in the panel —
 * there are no slash commands for background work, so the list has exactly one
 * presentation. Kept outside `startRepl` so the whole interaction can be driven
 * in tests without a terminal or a live agent.
 */

import type { BackgroundProcess } from '../../native/index.js'
import type { KeyEvent } from '../input.js'
import type { SelectorState } from '../selector.js'
import { selectorFocusOn } from '../selector.js'
import {
  backgroundProcessFingerprint,
  decideSessionSwitch,
  newlySettled,
  runningBackgroundCount,
  settledNoticeMessage,
  shouldWakeForNotifications,
  stopAllMessage,
  stopOneMessage,
} from './background-processes.js'
import {
  createBackgroundOutputState,
  createBackgroundPanelState,
  decideBackgroundPanelAction,
  focusedPanelTarget,
  isBackgroundOutputTitle,
  refreshBackgroundOutputState,
  refreshBackgroundPanelState,
  shouldDownOpenPanel,
} from './background-panel.js'

/** The slice of the native agent this controller needs. */
export interface BackgroundTerminalsClient {
  backgroundProcesses(sessionId: string): BackgroundProcess[]
  stopBackgroundProcess(sessionId: string, taskId: string): Promise<BackgroundProcess | null>
  stopAllBackgroundProcesses(sessionId: string): Promise<BackgroundProcess[]>
  backgroundForegroundProcesses(sessionId: string): number
  backgroundForegroundProcessesForMessage(sessionId: string): number
  blockingTaskWaits(sessionId: string): number
  pendingProcessNotifications(sessionId: string): number
  releaseBlockingTaskWaits(sessionId: string): number
  killAllBackgroundProcessesNow(): number
}

export interface BackgroundTerminalsDeps {
  client: BackgroundTerminalsClient
  /** Current session, or null while none is bound. */
  sessionId: () => string | null
  /** Commit one line of history. `slot` only labels the line's origin. */
  commit: (slot: string, text: string) => void
  requestRender: () => void
  /** Renders a caught error as text. */
  errorText: (err: unknown) => string
  /** Applies error styling; identity by default so tests read plain text. */
  paintError?: (text: string) => string
  /**
   * Reads the tail of a task's captured output file. Injected so tests stay off
   * disk, and so the caller decides how much of a large file to pull in.
   */
  readOutput: (path: string) => string
  /** Opens the panel as a selector overlay. */
  openPanel: (state: SelectorState) => void
  /** Replaces the open panel's state, or closes it when null. */
  updatePanel: (state: SelectorState | null) => void
  /** True while the panel is the active overlay. */
  panelOpen: () => boolean
  /** The panel's current state, or null when it is closed. */
  panelState: () => SelectorState | null
  /**
   * Opens a turn to deliver queued completion notices, if the host wants that.
   *
   * Optional so a host that has no notion of turns (tests, non-interactive
   * callers) simply never wakes. The controller supplies no text: the engine
   * puts the queued notices into the turn's input, so a synthetic user prompt
   * would only duplicate what the model is about to read.
   */
  wakeForNotifications?: () => void
  /** True while a run owns the turn. Such a run drains the queue itself. */
  runInFlight?: () => boolean
  /** Pending user messages, which will carry the notices when they submit. */
  queuedMessages?: () => number
  /** True while an overlay is waiting on the user (an ask, a prompt). */
  overlayBlocking?: () => boolean
}

export class BackgroundTerminals {
  private processes: BackgroundProcess[] = []
  /**
   * Blocking waits as of the last poll.
   *
   * Cached rather than read live because the spinner asks every frame (~100ms)
   * to decide whether to advertise ctrl+b, and each read crosses the native
   * boundary. Polled alongside the process list so both halves of
   * `canReclaimTurn()` come from one moment rather than two.
   */
  private blockingWaits = 0
  private warnedFor: string | null = null
  /**
   * Task ids whose settled outcome has already been reported.
   *
   * Prevents the 500ms poll from re-announcing the same transition, and lets a
   * panel-initiated stop claim its own id so the poll stays quiet about it.
   * Pruned against the live list, so it tracks the engine rather than growing
   * for the life of the session.
   */
  private readonly announced = new Set<string>()
  /**
   * Whether a wake may fire.
   *
   * Disarmed the moment one is requested and re-armed only when the queue is
   * observed empty. Without this, anything that makes `wakeForNotifications`
   * return without opening a turn — a signed-out cloud session, a run the host
   * declines to start — would leave the queue pending and the 500ms poll would
   * retry forever. It also means a wake is requested at most once per batch, so
   * the turn that drains the queue cannot race a second one.
   */
  private wakeArmed = true
  private readonly deps: BackgroundTerminalsDeps
  /**
   * The stop currently in flight, if any.
   *
   * Panel keypresses cannot await, so a stop is started as fire-and-forget.
   * Keeping the promise lets callers (and tests) wait for it to settle instead
   * of guessing at microtask counts.
   */
  private pending: Promise<void> = Promise.resolve()

  constructor(deps: BackgroundTerminalsDeps) {
    this.deps = deps
  }

  /** Resolves once any in-flight stop has settled and the list was refreshed. */
  async settled(): Promise<void> {
    await this.pending
  }

  /** Footer count: backgrounded work only. */
  runningCount(): number {
    return runningBackgroundCount(this.processes)
  }

  /** Shells still being waited on in the foreground. */
  foregroundCount(): number {
    return this.processes.filter(process => process.status === 'running_foreground').length
  }

  /**
   * Blocking `task_output` waits as of the last poll.
   *
   * Such a wait holds the turn while the task it watches is already
   * backgrounded, so `foregroundCount()` is zero and there is no shell to
   * detach. Counted separately so ctrl+b still has something to release when no
   * foreground shell exists.
   */
  blockingWaitCount(): number {
    return this.blockingWaits
  }

  /**
   * True while ctrl+b has something to move aside: either a shell is being
   * watched, or a blocking wait is holding the turn.
   *
   * Also gates the spinner hint, so the key is only advertised when it would do
   * something.
   */
  canReclaimTurn(): boolean {
    return this.foregroundCount() > 0 || this.blockingWaitCount() > 0
  }

  /**
   * End the waiting without touching the work.
   *
   * Detaches any foreground shell and releases any blocking `task_output` wait.
   * Both keep their processes alive, which is what lets ctrl+b promise never to
   * kill. Returns how many things stopped waiting.
   */
  reclaimTurn(): number {
    return this.backgroundForeground() + this.releaseBlockingWaits()
  }

  /**
   * Same release, so a queued message can reach the model.
   *
   * Steering is only inspected between tool calls, so anything holding the turn
   * holds the message with it — a foreground shell or a blocking `task_output`
   * call alike. Both are freed; the work keeps running.
   */
  reclaimTurnForMessage(): number {
    return this.backgroundForegroundForMessage() + this.releaseBlockingWaits()
  }

  private releaseBlockingWaits(): number {
    const sessionId = this.deps.sessionId()
    if (!sessionId) return 0
    try {
      const released = this.deps.client.releaseBlockingTaskWaits(sessionId)
      if (released > 0) {
        // Clear the cache now rather than waiting up to 500ms for the next poll:
        // otherwise a second ctrl+b within that window reads a stale non-zero
        // count, decides there is still something to release, and does nothing
        // visible.
        this.blockingWaits = 0
        this.deps.requestRender()
      }
      return released
    } catch {
      // A session can disappear mid-gesture; any detach still counts.
      return 0
    }
  }

  /**
   * Hand every foreground shell back as a background task.
   *
   * The processes keep running and their output files stay put; only the waiting
   * ends. This is what lets ctrl+b reclaim the turn non-destructively while a
   * long command is being watched — previously the only way out was to kill the
   * work.
   *
   * Returns how many moved, so the caller can tell whether the gesture applied.
   */
  backgroundForeground(): number {
    return this.detachForeground(sessionId =>
      this.deps.client.backgroundForegroundProcesses(sessionId),
    )
  }

  /**
   * Detach foreground shells so a queued message can reach the model.
   *
   * Steering is only inspected between tool calls, so a shell being watched in
   * the foreground holds a typed message until it finishes. Detaching lets the
   * message land while the command keeps running.
   */
  backgroundForegroundForMessage(): number {
    return this.detachForeground(sessionId =>
      this.deps.client.backgroundForegroundProcessesForMessage(sessionId),
    )
  }

  private detachForeground(detach: (sessionId: string) => number): number {
    const sessionId = this.deps.sessionId()
    if (!sessionId) return 0
    try {
      const moved = detach(sessionId)
      if (moved > 0) this.refresh()
      return moved
    } catch {
      // A session can disappear mid-gesture; treat it as nothing to move rather
      // than surfacing an error over a keypress.
      return 0
    }
  }

  /**
   * Re-read the task list, requesting a render only when something visible
   * moved. Elapsed time is excluded from the change key, so a ticking clock
   * alone never forces a repaint.
   */
  refresh(): void {
    const previous = this.processes
    const previousWaits = this.blockingWaits
    const sessionId = this.deps.sessionId()
    if (!sessionId) {
      this.processes = []
      this.blockingWaits = 0
      if (previous.length > 0 || previousWaits > 0) this.deps.requestRender()
      return
    }
    try {
      const next = this.deps.client.backgroundProcesses(sessionId)
      this.processes = next
      // Read in the same poll as the list so both halves of `canReclaimTurn()`
      // describe one moment. A live read per frame would cross the native
      // boundary at spinner rate for a value that changes rarely.
      //
      // Guarded separately: a failing probe must not abandon the rest of the
      // poll, or the panel and footer would freeze over a number that only
      // decides whether the ctrl+b hint is offered.
      try {
        this.blockingWaits = this.deps.client.blockingTaskWaits(sessionId)
      } catch {
        this.blockingWaits = 0
      }
      // Open list and output views are live: the list tracks status changes,
      // while the output view also re-reads the captured tail on every poll.
      if (this.deps.panelOpen()) {
        const state = this.deps.panelState()
        if (state) {
          if (isBackgroundOutputTitle(state.title)) {
            this.refreshOutputView(state, next)
          } else {
            this.deps.updatePanel(refreshBackgroundPanelState(state, next))
          }
        }
      }
      this.announceSettled(previous, next)
      if (
        backgroundProcessFingerprint(previous) !== backgroundProcessFingerprint(next)
        // The ctrl+b hint is derived from this, so a change has to repaint even
        // when the task list itself is unmoved.
        || previousWaits !== this.blockingWaits
      ) {
        this.deps.requestRender()
      }
      // Last in the poll: opening a turn re-enters the REPL, so every field
      // above is already settled before control leaves this method.
      this.maybeWake(sessionId)
    } catch {
      // A session can disappear during clear/delete/resume. Keep the last known
      // list and let the next poll recover rather than blanking the footer.
    }
  }

  /** Open the panel, or close either background view when already active. */
  togglePanel(): void {
    if (this.deps.panelOpen()) {
      this.deps.updatePanel(null)
      return
    }
    if (!this.deps.sessionId()) {
      this.deps.commit('none', '  No active session or background terminals.')
      return
    }
    this.refresh()
    this.deps.openPanel(createBackgroundPanelState(this.processes))
  }

  /**
   * Handle ↓ at the prompt: open the panel when the hint above the composer is
   * advertising it, otherwise decline so the key keeps its normal meaning.
   *
   * Returns true when the key was consumed. The caller must first give cursor
   * movement and history navigation their chance; only the leftover ↓ presses
   * reach here.
   */
  handlePromptDown(editorEmpty: boolean): boolean {
    if (!shouldDownOpenPanel({ editorEmpty, running: this.runningCount() })) return false
    this.togglePanel()
    return true
  }

  /** True when the prompt should advertise the panel gesture. */
  hintVisible(editorEmpty: boolean): boolean {
    return shouldDownOpenPanel({ editorEmpty, running: this.runningCount() })
  }

  /**
   * Handle a keypress while the panel is open.
   *
   * Returns true when the key was consumed. Navigation keys are left to the
   * generic selector handling; only the panel's own gestures are claimed here.
   */
  handlePanelKey(event: KeyEvent): boolean {
    const state = this.deps.panelState()
    if (!state) return false
    if (isBackgroundOutputTitle(state.title)) {
      if (event.type === 'escape') {
        const taskId = state.items[0]?.id
        const panel = createBackgroundPanelState(this.processes)
        this.deps.updatePanel(taskId ? selectorFocusOn(panel, item => item.id === taskId) : panel)
        return true
      }
      // Keep editor/navigation input out of the hidden composer, but let global
      // control chords (Ctrl+C, Ctrl+B, Ctrl+O, …) retain their REPL meaning.
      return event.type !== 'ctrl'
    }
    const action = decideBackgroundPanelAction(event, focusedPanelTarget(state, this.processes))
    switch (action.kind) {
      case 'none':
        return false
      case 'close':
        this.deps.updatePanel(null)
        return true
      case 'view':
        this.openOutputView(action.taskId)
        return true
      case 'stop':
        this.track(this.stopFromPanel(action.taskId))
        return true
      case 'stop-all':
        this.track(this.stopAllFromPanel())
        return true
    }
  }

  /** Record fire-and-forget work so `settled()` can await it. */
  private track(work: Promise<void>): void {
    // Chained rather than replaced: two quick stops must both be awaited.
    // Failures are already reported to the user inside the workers, so the
    // chain absorbs them here rather than staying permanently rejected.
    this.pending = this.pending.then(() => work).catch(() => {})
  }

  /** Open a full-width live tail without writing snapshots into history. */
  private openOutputView(taskId: string): void {
    const process = this.processes.find(candidate => candidate.task_id === taskId)
    if (!process) return
    this.deps.updatePanel(createBackgroundOutputState(process, this.readOutput(process)))
  }

  /** Refresh an open live tail from the latest process snapshot and file. */
  private refreshOutputView(state: SelectorState, processes: BackgroundProcess[]): void {
    const taskId = state.items[0]?.id
    const process = processes.find(candidate => candidate.task_id === taskId)
    if (!process) {
      this.deps.updatePanel(createBackgroundPanelState(processes))
      return
    }
    const next = refreshBackgroundOutputState(state, process, this.readOutput(process))
    const currentItem = state.items[0]
    const nextItem = next.items[0]
    const unchanged = state.subtitle === next.subtitle
      && currentItem?.detail === nextItem?.detail
      && currentItem?.preview?.length === nextItem?.preview?.length
      && currentItem?.preview?.every((line, index) => line === nextItem?.preview?.[index])
    if (!unchanged) this.deps.updatePanel(next)
  }

  private readOutput(process: BackgroundProcess): string {
    try {
      return this.deps.readOutput(process.output_path)
    } catch (err) {
      return `(could not read output: ${this.deps.errorText(err)})`
    }
  }

  /** Stop one task from the panel, reporting the outcome in the transcript. */
  private async stopFromPanel(taskId: string): Promise<void> {
    const sessionId = this.deps.sessionId()
    if (!sessionId) return
    try {
      const stopped = await this.deps.client.stopBackgroundProcess(sessionId, taskId)
      // Claimed before the refresh below observes the transition: the panel is
      // reporting this stop itself, and the poll must not say it again.
      this.announced.add(taskId)
      if (stopped) this.deps.commit('stop', stopOneMessage(stopped))
    } catch (err) {
      this.deps.commit('error', this.paint(`  Could not stop ${taskId.slice(0, 8)}: ${this.deps.errorText(err)}`))
    }
    this.refresh()
  }

  private async stopAllFromPanel(): Promise<void> {
    const sessionId = this.deps.sessionId()
    if (!sessionId) return
    try {
      const stopped = await this.deps.client.stopAllBackgroundProcesses(sessionId)
      for (const process of stopped) this.announced.add(process.task_id)
      this.deps.commit('stop-all', stopAllMessage(stopped.length))
    } catch (err) {
      this.deps.commit('error', this.paint(`  Could not stop background terminals: ${this.deps.errorText(err)}`))
    }
    this.refresh()
  }

  /**
   * Report tasks that settled since the last poll, once each.
   *
   * The engine already queues an equivalent notification for the model, so this
   * is the user's half of the same event: a task finishing while the agent is
   * idle otherwise changes nothing on screen but the footer count.
   */
  private announceSettled(previous: BackgroundProcess[], next: BackgroundProcess[]): void {
    for (const process of newlySettled(previous, next)) {
      if (this.announced.has(process.task_id)) continue
      this.announced.add(process.task_id)
      this.deps.commit('settled', settledNoticeMessage(process))
    }
    // Forget ids the engine has reclaimed so the set cannot grow without bound
    // over a long session. A reclaimed task can never transition again.
    if (this.announced.size > 0) {
      const live = new Set(next.map(process => process.task_id))
      for (const id of this.announced) {
        if (!live.has(id)) this.announced.delete(id)
      }
    }
  }

  private paint(text: string): string {
    return (this.deps.paintError ?? ((value: string) => value))(text)
  }

  /**
   * Open a turn when a finished task left a result nobody will receive.
   *
   * This is what keeps a multi-step instruction alive across a long task: the
   * engine queues the completion notice, and without a turn to carry it the
   * queue sits untouched until the user types, so "run the build, then fix what
   * breaks" would stop after the build.
   *
   * The queue itself is the trigger, not the settled transition, so this fires
   * exactly once per batch — `build_turn` drains it, and the next poll sees
   * zero. A host that supplies no `wakeForNotifications` never wakes.
   */
  private maybeWake(sessionId: string): void {
    const wake = this.deps.wakeForNotifications
    if (!wake) return
    // Guarded like the blocking-wait probe: a failing count must not abandon
    // the poll, and must not be read as "something is pending".
    let pending = 0
    try {
      pending = this.deps.client.pendingProcessNotifications(sessionId)
    } catch {
      return
    }
    // Re-arm only once the engine confirms the queue is empty, which is the one
    // signal that a turn actually took delivery.
    if (pending === 0) {
      this.wakeArmed = true
      return
    }
    if (!this.wakeArmed) return
    const ready = shouldWakeForNotifications({
      pending,
      hasSession: true,
      runInFlight: this.deps.runInFlight?.() ?? false,
      queuedMessages: this.deps.queuedMessages?.() ?? 0,
      overlayBlocking: this.deps.overlayBlocking?.() ?? false,
    })
    if (!ready) return
    // Disarmed before the call, not after: opening a turn re-enters the REPL
    // synchronously, so a later assignment could be reached after a nested poll.
    this.wakeArmed = false
    wake()
  }

  /**
   * Gate a command that would leave the current session behind.
   *
   * Returns true when the command must not run. The first attempt warns and the
   * next identical command proceeds, so a task that ignores SIGKILL can never
   * trap the user in the session.
   */
  guardSessionSwitch(command: string): boolean {
    if (!this.deps.sessionId()) return false
    this.refresh()
    const decision = decideSessionSwitch({
      command,
      running: this.runningCount(),
      warnedFor: this.warnedFor,
    })
    if (decision.kind === 'warn') {
      this.warnedFor = command
      this.deps.commit('session-switch', decision.message)
      this.deps.requestRender()
      return true
    }
    this.warnedFor = null
    return false
  }

  /**
   * Kill every background process synchronously. Used on exit paths that call
   * `fastExit`, which skips the async teardown that would otherwise stop them.
   */
  killAllNow(): number {
    try {
      return this.deps.client.killAllBackgroundProcessesNow()
    } catch {
      // Never block shutdown on cleanup of processes we are abandoning anyway.
      return 0
    }
  }
}
