/**
 * Background task panel: an interactive list of background terminals.
 *
 * Replaces the old line dump. Rows are selectable, a task can be stopped
 * in place, and its captured output can be pulled into the transcript — so
 * managing background work never requires typing a task id.
 *
 * Everything here is pure so the whole interaction is testable without a
 * terminal: the controller in `background-terminals.ts` owns the side effects.
 */

import type { KeyEvent } from '../input.js'
import type { BackgroundProcess } from '../../native/index.js'
import type { Hint } from './hint.js'
import { createSelectorState, type SelectorItem, type SelectorState } from '../selector.js'
import { formatElapsed } from '../../render/format.js'

export const BACKGROUND_PANEL_TITLE = 'Background'

/**
 * Gesture advertised at the prompt while background work is live.
 *
 * ↓ is the primary way in: the hint sits directly above the composer, so the
 * key it names should be reachable without a modifier.
 */
export const BACKGROUND_PANEL_HINT_CHORD = '↓'

/** Panel toggle key. Ctrl+T is a portable C0 control character and unbound. */
export const BACKGROUND_PANEL_SHORTCUT_HINT = 'ctrl+t'

export function isBackgroundPanelShortcut(event: KeyEvent): boolean {
  return event.type === 'ctrl' && event.key === 't'
}

/**
 * Whether ↓ should open the panel instead of doing nothing.
 *
 * ↓ keeps every meaning it already had: the caller only consults this once
 * cursor movement and history navigation have both declined the key. The
 * remaining cases are gated on an empty composer, so ↓ never diverts attention
 * mid-sentence, and on live work, so the key does something only while the
 * prompt is actually advertising it.
 */
export function shouldDownOpenPanel(input: { editorEmpty: boolean; running: number }): boolean {
  return input.editorEmpty && input.running > 0
}

/** True when a selector is the background panel. */
export function isBackgroundPanelTitle(title: string): boolean {
  return title === BACKGROUND_PANEL_TITLE
}

/** Statuses that are still doing work. */
export function isLiveStatus(status: BackgroundProcess['status']): boolean {
  return status === 'running' || status === 'running_foreground'
}

/**
 * Compact elapsed clock. Seconds below a minute, then `1m 3s` — a background
 * task often runs for minutes, where `94.0s` stops being readable.
 *
 * Defined in `render/format.ts` so the task tool cards spell a runtime exactly
 * the same way this panel does; re-exported here because the panel is where
 * callers expect to find it.
 */
export { formatElapsed }

/** First non-empty line of a command, with the rest reported as a line count. */
export function formatCommandLabel(command: string, maxChars = 96): string {
  const lines = command.split('\n')
  const first = lines.map(line => line.trim()).find(Boolean) ?? '(empty)'
  const extra = lines.length - 1
  const suffix = extra > 0 ? ` (+${extra} ${extra === 1 ? 'line' : 'lines'})` : ''
  const available = Math.max(1, maxChars - suffix.length)
  if (first.length <= available) return `${first}${suffix}`
  return `${first.slice(0, Math.max(1, available - 1))}…${suffix}`
}

/** Parenthesised status shown after a row's command. */
export function formatStatusDetail(process: BackgroundProcess): string {
  const elapsed = formatElapsed(process.elapsed_ms)
  const exit = process.exit_code === null ? '' : `exit ${process.exit_code} · `
  switch (process.status) {
    case 'running':
    case 'running_foreground':
      return `running · ${elapsed}`
    case 'completed':
      return `${exit}${elapsed}`
    case 'failed':
      return `failed · ${exit}${elapsed}`
    case 'killed':
      // "cancelled" when the user asked for it, matching the task tool cards:
      // the same task must not read as `stopped` here and `cancelled by user`
      // there.
      return process.stopped_by_user
        ? `cancelled by user · ${elapsed}`
        : `stopped · ${elapsed}`
  }
}

/** Body text when nothing has run in this session yet. */
export const PANEL_EMPTY_MESSAGE = 'No tasks currently running'

/**
 * Count line under the title. Counts live tasks only, finished ones separately.
 *
 * "active" is deliberately broader than the footer chip's "background shells
 * running": this counts `running` and `running_foreground` alike, because the
 * panel lists both, whereas the chip counts only detached shells. The two lines
 * can therefore disagree by design, and the wording is what keeps that honest.
 *
 * An empty list has no subtitle: the body already says there is nothing to show,
 * and repeating it two rows apart reads like two different statements.
 */
export function formatPanelSubtitle(processes: BackgroundProcess[]): string | undefined {
  if (processes.length === 0) return undefined
  const live = processes.filter(process => isLiveStatus(process.status)).length
  const done = processes.length - live
  const parts: string[] = []
  if (live > 0) parts.push(`${live} active ${live === 1 ? 'shell' : 'shells'}`)
  if (done > 0) parts.push(`${done} finished`)
  return parts.join(' · ')
}

/** Group labels. Live work sits above finished work, newest task last. */
export const SHELLS_GROUP = 'Shells'
export const COMPLETED_GROUP = 'Completed'

/**
 * Selector rows for the panel, live tasks first and finished tasks below.
 *
 * A single group carries no heading: with only running shells on screen the
 * label would state what the panel title already says. The heading appears as
 * soon as the list splits, where it earns its row by separating the two.
 */
export function formatPanelItems(processes: BackgroundProcess[]): SelectorItem[] {
  const live = processes.filter(process => isLiveStatus(process.status))
  const done = processes.filter(process => !isLiveStatus(process.status))
  const running = live.length

  const row = (process: BackgroundProcess, group: string): SelectorItem => ({
    label: formatCommandLabel(process.command),
    detail: `(${formatStatusDetail(process)})`,
    id: process.task_id,
    group,
    // Hints ride on the row so moving the cursor onto a finished task drops
    // `x to stop` without any extra bookkeeping.
    hints: backgroundPanelHints(
      { id: process.task_id, live: isLiveStatus(process.status) },
      running,
    ),
    // The full command and id stay searchable even though the row is truncated.
    searchText: `${process.command} ${process.task_id}`,
  })

  const split = live.length > 0 && done.length > 0
  const header = (label: string, size: number): SelectorItem[] =>
    split ? [{ label, headerCount: size, header: true, focusable: false, group: label }] : []

  return [
    ...(live.length > 0 ? [...header(SHELLS_GROUP, live.length), ...live.map(p => row(p, SHELLS_GROUP))] : []),
    ...(done.length > 0 ? [...header(COMPLETED_GROUP, done.length), ...done.map(p => row(p, COMPLETED_GROUP))] : []),
  ]
}

/** Open the panel over the current task list. */
export function createBackgroundPanelState(processes: BackgroundProcess[]): SelectorState {
  return {
    ...createSelectorState(BACKGROUND_PANEL_TITLE, formatPanelItems(processes)),
    subtitle: formatPanelSubtitle(processes),
    noFilter: true,
    emptyMessage: PANEL_EMPTY_MESSAGE,
    // Fallback for an empty list, where no row can carry hints.
    hints: backgroundPanelHints(null, 0),
  }
}

/**
 * Refresh an open panel in place.
 *
 * Focus follows the task id rather than the row index, so a task finishing or
 * being removed under the cursor cannot silently redirect the next keypress at
 * a different task. A finishing task moves between groups, which shifts every
 * index around it — matching on id is what keeps the cursor on the same shell.
 */
export function refreshBackgroundPanelState(
  state: SelectorState,
  processes: BackgroundProcess[],
): SelectorState {
  const focusedId = state.items[state.focusIndex]?.id
  const items = formatPanelItems(processes)
  const next: SelectorState = {
    ...state,
    items,
    allItems: items,
    subtitle: formatPanelSubtitle(processes),
    scrollOffset: Math.min(state.scrollOffset, Math.max(0, items.length - 1)),
  }
  const focusIndex = focusedId ? items.findIndex(item => item.id === focusedId) : -1
  if (focusIndex >= 0) return { ...next, focusIndex }
  // The focused task disappeared. Clamp onto the nearest surviving row, skipping
  // group headings: they are not focusable, and landing on one would leave the
  // panel with no actionable selection.
  return { ...next, focusIndex: nearestFocusable(items, state.focusIndex) }
}

/** The closest focusable row at or before `index`, else the first one. */
function nearestFocusable(items: SelectorItem[], index: number): number {
  const clamped = Math.min(index, Math.max(0, items.length - 1))
  for (let i = clamped; i >= 0; i--) {
    if (items[i] && items[i]!.focusable !== false) return i
  }
  const forward = items.findIndex(item => item.focusable !== false)
  return forward >= 0 ? forward : 0
}

export type BackgroundPanelAction =
  | { kind: 'view'; taskId: string }
  | { kind: 'stop'; taskId: string }
  | { kind: 'stop-all' }
  | { kind: 'close' }
  | { kind: 'none' }

/**
 * Map a keypress inside the panel onto an action.
 *
 * `enter` views output, `x` stops the focused task, `X` stops every task, and
 * `esc` closes. Stopping an already-finished task is a no-op rather than an
 * error: its process is gone, so there is nothing to signal.
 *
 * The panel deliberately has no type-to-filter: a session holds at most a
 * handful of shells, and reserving bare letters for actions keeps `x` free for
 * the destructive gesture instead of hiding it behind a modifier.
 */
export function decideBackgroundPanelAction(
  event: KeyEvent,
  focused: { id: string; live: boolean } | null,
): BackgroundPanelAction {
  if (event.type === 'escape') return { kind: 'close' }
  if (event.type === 'enter') {
    return focused ? { kind: 'view', taskId: focused.id } : { kind: 'none' }
  }
  // Shift+X arrives as `shift-char` under the kitty protocol (already
  // lowercased) and as a bare uppercase `char` on legacy terminals. Both mean
  // stop-all, so both are accepted.
  if (event.type === 'shift-char' && event.char.toLowerCase() === 'x') return { kind: 'stop-all' }
  if (event.type === 'char' && event.char === 'X') return { kind: 'stop-all' }
  if (event.type === 'char' && event.char === 'x') {
    if (!focused || !focused.live) return { kind: 'none' }
    return { kind: 'stop', taskId: focused.id }
  }
  return { kind: 'none' }
}

/** The focused row as an action target, or null when the list is empty. */
export function focusedPanelTarget(
  state: SelectorState,
  processes: BackgroundProcess[],
): { id: string; live: boolean } | null {
  const id = state.items[state.focusIndex]?.id
  if (!id) return null
  const process = processes.find(candidate => candidate.task_id === id)
  return { id, live: process ? isLiveStatus(process.status) : false }
}

/**
 * Footer hints for the panel, matching the gestures that are live right now.
 *
 * Every entry is conditional on being usable: `↑/↓ to select` needs a row to
 * move onto, `x to stop` needs the focused row to be running, and
 * `X to stop all` only earns its place when more than one shell is live (with
 * exactly one, it duplicates `x`). A hint for a key that does nothing is worse
 * than no hint, because pressing it is the only way to find out.
 */
export function backgroundPanelHints(
  focused: { id: string; live: boolean } | null,
  runningCount: number,
): Hint[] {
  return [
    ...(focused ? [{ keys: ['up', 'down'], action: 'select' } satisfies Hint] : []),
    ...(focused ? [{ keys: 'enter', action: 'view output' } satisfies Hint] : []),
    ...(focused?.live ? [{ keys: 'x', action: 'stop' } satisfies Hint] : []),
    ...(runningCount > 1 ? [{ keys: 'X', action: 'stop all' } satisfies Hint] : []),
    { keys: 'escape', action: 'close' },
  ]
}

/** Output lines the panel pulls into the transcript for one task. */
export const OUTPUT_TAIL_LINES = 40

/**
 * Transcript block for `enter` on a task: a header naming the task, then the
 * tail of its captured output.
 *
 * The tail is used rather than the head because a running task's interesting
 * state is its latest output. Truncation is always stated so a partial view is
 * never mistaken for the whole run, and the output path is included so the full
 * file stays reachable.
 */
export function formatOutputView(
  process: BackgroundProcess,
  output: string,
  tailLines = OUTPUT_TAIL_LINES,
): string[] {
  const id = process.task_id.slice(0, 8)
  const header = `  ${STATUS_MARK[process.status]} ${id} · ${formatStatusDetail(process)}  ${formatCommandLabel(process.command)}`
  // A trailing newline is a line terminator, not an empty final line.
  const normalized = output.endsWith('\n') ? output.slice(0, -1) : output
  const all = normalized.length === 0 ? [] : normalized.split('\n')
  if (all.length === 0) {
    return [header, '    (no output yet)', `    ${process.output_path}`]
  }
  const visible = all.slice(-tailLines)
  const hidden = all.length - visible.length
  return [
    header,
    ...(process.output_file_truncated
      ? ['    (output file was capped; earlier output was dropped)']
      : []),
    ...(hidden > 0 ? [`    … ${hidden} earlier ${hidden === 1 ? 'line' : 'lines'}`] : []),
    ...visible.map(line => `    ${line}`),
    `    ${process.output_path}`,
  ]
}

const STATUS_MARK: Record<BackgroundProcess['status'], string> = {
  running_foreground: '●',
  running: '●',
  completed: '✓',
  failed: '✗',
  killed: '■',
}
