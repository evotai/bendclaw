import { dim, line, colored, type ViewBlock } from './types.js'
import stringWidth from 'string-width'
import { truncateToWidth } from './width.js'

export interface AdContent {
  id: string
  kind: 'notice' | 'ad'
  priority?: number
  title: string
  body: string
}

/** Visual phase of the slot, driven by elapsed time. */
export type AdSlotPhase = 'entering' | 'steady' | 'erasing' | 'gone'

export interface AdSlotState {
  notices: AdContent[]
  ads: AdContent[]
  /** Notices already shown once; they stop jumping the queue afterwards. */
  seenNoticeIds: Set<string>
  /** False until the first trigger; the slot stays hidden before that. */
  triggered: boolean
  currentId: string | null
  /** Epoch ms when the current content started showing. */
  shownAt: number
  /** Epoch ms when rotation is due. */
  rotationDueAt: number
  /**
   * Next content queued. While set, the current line erases away, holds blank
   * for AD_GAP_MS, then this item types itself in.
   */
  queuedId: string | null
}

// ---- timing (ms) -----------------------------------------------------------

/** How long one piece of content stays before rotating out. */
export const AD_STEADY_MS = 45_000
/** Enter animation length. */
export const AD_ENTER_MS = 400

export function createAdSlotState(notices: AdContent[]): AdSlotState {
  return {
    notices: notices.filter(n => n.kind === 'notice'),
    ads: notices.filter(n => n.kind === 'ad'),
    seenNoticeIds: new Set(),
    triggered: false,
    currentId: null,
    shownAt: 0,
    rotationDueAt: 0,
    queuedId: null,
  }
}

function byId(state: AdSlotState, id: string | null): AdContent | null {
  if (!id) return null
  return state.notices.find(n => n.id === id) ?? state.ads.find(a => a.id === id) ?? null
}

function nextNotice(state: AdSlotState): AdContent | null {
  return state.notices
    .filter(n => !state.seenNoticeIds.has(n.id))
    .sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0))[0] ?? null
}

/**
 * Every campaign in a stable running order: notices by priority, then ads in
 * server order. Rotation walks this list and wraps, so the slot cycles for as
 * long as the session lives instead of dead-ending on the last item.
 */
function playlist(state: AdSlotState): AdContent[] {
  const notices = [...state.notices].sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0))
  return [...notices, ...state.ads]
}

/** The item after `current` in the playlist, wrapping at the end. */
function pickFollowUp(state: AdSlotState, current: AdContent): AdContent {
  // An unseen notice jumps the queue so announcements land promptly.
  const fresh = state.notices
    .filter(n => n.id !== current.id && !state.seenNoticeIds.has(n.id))
    .sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0))[0]
  if (fresh) return fresh

  const all = playlist(state)
  if (all.length === 0) return current
  const at = all.findIndex(c => c.id === current.id)
  return all[(at + 1) % all.length] ?? current
}

function pin(state: AdSlotState, content: AdContent, now: number): void {
  state.currentId = content.id
  state.shownAt = now
  state.rotationDueAt = now + AD_STEADY_MS
}

/** Mark `content` as shown so it stops jumping the rotation queue. */
function retireCurrent(state: AdSlotState, content: AdContent): void {
  if (content.kind === 'notice') state.seenNoticeIds.add(content.id)
}

/**
 * Frame hook driving the whole lifecycle:
 *   entering → steady → erasing → (gap) → next content entering → …
 * Hidden until the first trigger, then it keeps showing for the session.
 * Content cycles the playlist endlessly, wrapping after the last item.
 * An unseen notice jumps the queue through the same erase transition, so
 * content never hard-cuts.
 */
export function tickAdSlot(state: AdSlotState, now: number): { content: AdContent | null; phase: AdSlotPhase; progress: number } {
  if (!state.triggered) return { content: null, phase: 'gone', progress: 0 }

  const freshNotice = nextNotice(state)
  const content = byId(state, state.currentId)
  if (!content) {
    const next = freshNotice ?? playlist(state)[0] ?? null
    if (!next) return { content: null, phase: 'gone', progress: 0 }
    pin(state, next, now)
    return enterFrame(state, next, now)
  }

  // Erase-then-type transition when something is queued: the current text is
  // wiped one character at a time, holds blank for AD_GAP_MS, then the next
  // item types itself in.
  const queued = byId(state, state.queuedId)
  if (queued) {
    const elapsed = now - state.shownAt
    const eraseDoneAt = fullText(content).length * ERASE_STEP_MS
    if (elapsed < eraseDoneAt) {
      return { content, phase: 'erasing', progress: 1 - elapsed / eraseDoneAt }
    }
    if (elapsed < eraseDoneAt + AD_GAP_MS) {
      // Blank beat between items so they don't run together.
      return { content, phase: 'erasing', progress: 0 }
    }
    retireCurrent(state, content)
    state.queuedId = null
    pin(state, queued, now)
    return enterFrame(state, queued, now)
  }

  const settled = now - state.shownAt > AD_ENTER_MS * 3
  const rotationDue = now >= state.rotationDueAt
  // A fresh notice outranks a showing ad, but only once the ad has settled so
  // a just-typed line isn't yanked away.
  const preempt = content.kind === 'ad' && freshNotice !== null && settled
  if (rotationDue || preempt) {
    const followUp = preempt ? freshNotice! : pickFollowUp(state, content)
    if (followUp.id !== content.id) {
      // Start the erase on this frame; the transition completes on later ones.
      queueTransition(state, followUp.id, now)
      return { content, phase: 'erasing', progress: 1 }
    }
    pin(state, followUp, now)   // sole campaign: retype it in place
    return enterFrame(state, followUp, now)
  }

  const age = now - state.shownAt
  if (age <= AD_ENTER_MS) return { content, phase: 'entering', progress: age / AD_ENTER_MS }
  return { content, phase: 'steady', progress: Math.min(1, age / AD_STEADY_MS) }
}

function enterFrame(state: AdSlotState, content: AdContent, now: number) {
  const age = now - state.shownAt
  if (age >= AD_ENTER_MS) return { content, phase: 'steady' as const, progress: age / AD_STEADY_MS }
  return { content, phase: 'entering' as const, progress: age / AD_ENTER_MS }
}

/** Queue a transition: erase the current line, then type the next content. */
function queueTransition(state: AdSlotState, nextId: string, now: number): void {
  state.queuedId = nextId
  state.shownAt = now   // reuse shownAt as the phase clock
}

/**
 * Publicly queue `id` as the next content. Used when a fresh campaign arrives
 * mid-session: whatever shows now erases away, then the new item types in.
 * No-op when the slot is not showing anything or `id` is already current.
 */
export function queueAdSlotTransition(state: AdSlotState, id: string, now = Date.now()): boolean {
  if (state.currentId === null || state.currentId === id) return false
  if (byId(state, id) === null) return false
  queueTransition(state, id, now)
  return true
}

/**
 * Event trigger: reveal the slot and start (or resume) the rotation. Called
 * after login and on task completion. Returns the content that will show.
 */
export function triggerAdSlot(state: AdSlotState, now: number): AdContent | null {
  // Resume whatever was showing; otherwise start at the head of the playlist.
  const resume = byId(state, state.currentId)
  const content = nextNotice(state) ?? resume ?? playlist(state)[0] ?? null
  if (!content) return null
  if (resume && resume.id !== content.id) retireCurrent(state, resume)
  state.queuedId = null
  state.triggered = true
  pin(state, content, now)
  return content
}

// ---- typewriter (single line, types out then holds) ------------------------

/** How often one character is revealed, in ms. */
export const TYPE_STEP_MS = 35
/** How fast the eraser removes characters, in ms per character. */
export const ERASE_STEP_MS = 45
/** Blank pause between erasing one item and typing the next. */
export const AD_GAP_MS = 900

/**
 * The line's text: title and body, nothing else — no kind label, no heading
 * above or below the rules.
 */
function fullText(content: AdContent): string {
  return [content.title, content.body].filter(Boolean).join('   \u00b7   ')
}

/** Characters visible at `now`, given when typing started. */
export function typedLength(shownAt: number, now: number): number {
  return Math.max(0, Math.floor((now - shownAt) / TYPE_STEP_MS))
}

/**
 * The slot: a single line that types itself out character by character, then
 * holds until rotation. Minimal footprint, calm motion.
 */
export function buildAdSlotBlocks(
  state: AdSlotState,
  tick: { content: AdContent | null; phase: AdSlotPhase; progress: number },
  columns: number,
  now: number = Date.now(),
): ViewBlock[] {
  const { content, phase } = tick
  if (!content || phase === 'gone' || columns < 30) return []

  // Rule width covers the widest campaign in the pool — stable across
  // rotations instead of resizing per item.
  const widest = [...state.notices, ...state.ads]
    .reduce((max, c) => Math.max(max, stringWidth(fullText(c))), 0)
  const innerWidth = Math.max(20, Math.min(widest + 3, columns - 6))
  const rule = colored('  ' + '─'.repeat(innerWidth), 'yellow')

  const full = fullText(content)
  let shown: string
  if (phase === 'erasing') {
    // Wipe one character at a time from the end, then hold blank.
    const kept = Math.max(0, full.length - Math.floor((now - state.shownAt) / ERASE_STEP_MS))
    shown = full.slice(0, kept)
  } else {
    shown = full.slice(0, typedLength(state.shownAt, now))
  }
  const visible = truncateToWidth(shown, innerWidth - 1)

  return [{
    lines: [
      line(rule),
      line(colored(' ', 'yellow'), dim(' '), colored(visible, 'white')),
      line(rule),
    ],
    marginTop: 1,
  }]
}
