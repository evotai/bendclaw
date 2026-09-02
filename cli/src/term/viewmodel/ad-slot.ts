import { line, plain, type StyledSpan, type ViewBlock } from './types.js'
import { renderMarkdown } from '../../render/markdown.js'
import { sliceVisibleAnsi, splitAnsiIntoCells, truncateAnsiToWidth, visibleGraphemeCount, visibleWidth } from '../../render/wrap.js'
import { getTheme } from '../../render/theme.js'

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
  /** Granted (premium) model: no ads, and notices announce rather than banner. */
  premium: boolean
  /** Copy shown this session, so the 15s sync cannot replay unchanged copy. */
  shownFingerprints: Set<string>
}

export function campaignFingerprint(
  campaign: { id: string; kind: string; priority?: number; title: string; body: string },
): string {
  return `${campaign.id}\0${campaign.kind}\0${campaign.priority ?? 0}\0${campaign.title}\0${campaign.body}`
}

export interface AdSlotOptions {
  premium?: boolean
  shownFingerprints?: Iterable<string>
}

// ---- timing (ms) -----------------------------------------------------------

/** How long one piece of content stays before rotating out. */
export const AD_STEADY_MS = 45_000
/** Enter animation length. */
export const AD_ENTER_MS = 400

export function createAdSlotState(
  notices: AdContent[],
  options: AdSlotOptions = {},
): AdSlotState {
  const premium = options.premium ?? false
  const shown = new Set(options.shownFingerprints ?? [])
  const admitted = premium
    ? notices.filter(n => n.kind === 'notice' && !shown.has(campaignFingerprint(n)))
    : notices
  return {
    notices: admitted.filter(n => n.kind === 'notice'),
    ads: admitted.filter(n => n.kind === 'ad'),
    seenNoticeIds: new Set(),
    triggered: false,
    currentId: null,
    shownAt: 0,
    rotationDueAt: 0,
    queuedId: null,
    premium,
    shownFingerprints: shown,
  }
}

function byId(state: AdSlotState, id: string | null): AdContent | null {
  if (!id) return null
  return state.notices.find(n => n.id === id) ?? state.ads.find(a => a.id === id) ?? null
}

function nextNotice(state: AdSlotState): AdContent | null {
  // Premium gates on copy, not id: an edit keeps the id.
  const unseen = state.premium
    ? (n: AdContent) => !state.shownFingerprints.has(campaignFingerprint(n))
    : (n: AdContent) => !state.seenNoticeIds.has(n.id)
  return state.notices
    .filter(unseen)
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
    // No playlist fallback for premium: it ignores shown copy.
    const next = state.premium ? freshNotice : (freshNotice ?? playlist(state)[0] ?? null)
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
    const eraseDoneAt = Math.max(1, visibleGraphemeCount(tickerText(content))) * ERASE_STEP_MS
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
    // Premium shows a notice once, then goes quiet.
    if (state.premium) {
      retireCurrent(state, content)
      state.shownFingerprints.add(campaignFingerprint(content))
      state.currentId = null
      state.queuedId = null
      const nextFresh = nextNotice(state)
      if (!nextFresh) return { content: null, phase: 'gone', progress: 0 }
      pin(state, nextFresh, now)
      return enterFrame(state, nextFresh, now)
    }
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
  const resume = byId(state, state.currentId)
  // Every sync re-triggers the slot; the fallback would bring back shown copy.
  const fallback = state.premium ? null : playlist(state)[0] ?? null
  const content = nextNotice(state) ?? resume ?? fallback
  if (!content) return null
  if (resume && resume.id !== content.id) retireCurrent(state, resume)
  state.queuedId = null
  state.triggered = true
  pin(state, content, now)
  return content
}

// ---- typewriter (markdown body, types out then holds) ----------------------

/** How often one character is revealed, in ms. */
export const TYPE_STEP_MS = 35
/** How fast the eraser removes characters, in ms per character. */
export const ERASE_STEP_MS = 45
/** Blank pause between erasing one item and typing the next. */
export const AD_GAP_MS = 900

/** Title plus markdown body. Parsed as markdown, then flattened to one ticker
 *  line so the slot never grows past a single row. */
function sourceMarkdown(content: AdContent): string {
  const title = content.title.trim()
  const body = content.body.trim()
  if (title && body) return `${title}\n\n${body}`
  return title || body
}

const renderedCache = new Map<string, string>()

function renderedMarkdown(content: AdContent): string {
  const source = sourceMarkdown(content)
  if (!source) return ''
  const hit = renderedCache.get(source)
  if (hit !== undefined) return hit
  let rendered: string
  try {
    rendered = renderMarkdown(source, { blockSpacing: 'compact' })
  } catch {
    rendered = source
  }
  if (renderedCache.size > 64) {
    const first = renderedCache.keys().next().value
    if (first !== undefined) renderedCache.delete(first)
  }
  renderedCache.set(source, rendered)
  return rendered
}

function flattenToOneLine(rendered: string): string {
  return rendered
    .split(/\r\n|\r|\n/)
    .map(row => row.trimEnd())
    .filter(row => visibleWidth(row) > 0)
    .join('   \u00b7   ')
}

function tickerText(content: AdContent): string {
  return flattenToOneLine(renderedMarkdown(content))
}

function campaignWidth(content: AdContent): number {
  return visibleWidth(tickerText(content))
}

/** Characters visible at `now`, given when typing started. */
export function typedLength(shownAt: number, now: number): number {
  return Math.max(0, Math.floor((now - shownAt) / TYPE_STEP_MS))
}

function revealMarkdown(rendered: string, keep: number): string {
  const total = visibleGraphemeCount(rendered)
  if (keep <= 0) return ''
  if (keep >= total) return rendered
  return sliceVisibleAnsi(rendered, keep)
}

/**
 * Dark tints for the slot's dithered fill, derived from the theme's selection
 * hex so the band tracks light/dark without new theme fields. The spread is
 * deliberately narrow: enough texture to mark the slot as its own surface,
 * not enough to compete with the copy sitting on it.
 */
function bandTints(): string[] {
  const base = getTheme().selectionBgHex
  const r = Number.parseInt(base.slice(1, 3), 16)
  const g = Number.parseInt(base.slice(3, 5), 16)
  const b = Number.parseInt(base.slice(5, 7), 16)
  if (!Number.isFinite(r) || !Number.isFinite(g) || !Number.isFinite(b)) return [base]
  const hex = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0')
  const shift = (f: number) => `#${hex(r * f)}${hex(g * f)}${hex(b * f)}`
  return [shift(0.78), shift(0.88), shift(1), shift(0.92)]
}

/**
 * Deterministic per-column noise in [0,1).
 *
 * Hashed rather than random on purpose: the TUI repaints on every spinner tick
 * and keystroke, so a random field would crawl and flicker. The same column
 * always lands on the same tint, making the band hold still.
 */
function columnNoise(column: number): number {
  let x = Math.imul(column + 1, 2654435761)
  x ^= x >>> 15
  x = Math.imul(x, 2246822507)
  x ^= x >>> 13
  return (x >>> 0) / 4294967296
}

/**
 * Lay the ticker over a dithered band of `width` columns.
 *
 * Splitting into one span per column is what makes the fill possible: `bg`
 * wraps each cell last, so it survives the foreground colours markdown already
 * baked into the text, and trailing blanks get the same treatment as the copy.
 */
function bandSpans(rendered: string, width: number): StyledSpan[] {
  const tints = bandTints()
  const cells = splitAnsiIntoCells(rendered)
  const spans: StyledSpan[] = []
  for (let column = 0; column < width; column++) {
    const cell = cells[column]
    // Filler entries for the second half of a wide glyph carry no text; they
    // must not become a padded space or the row would drift a column.
    if (cell === '') continue
    const tint = tints[Math.floor(columnNoise(column) * tints.length)]
    spans.push({ text: cell ?? ' ', bg: tint })
  }
  return spans
}

/**
 * The slot: a single markdown ticker on a dithered band. Typed in character by
 * character, then held until rotation. Never wraps — overflow is truncated.
 */
export function buildAdSlotBlocks(
  state: AdSlotState,
  tick: { content: AdContent | null; phase: AdSlotPhase; progress: number },
  columns: number,
  now: number = Date.now(),
): ViewBlock[] {
  const { content, phase } = tick
  if (!content || phase === 'gone' || columns < 30) return []

  const innerWidth = Math.max(20, Math.min(
    [...state.notices, ...state.ads].reduce((max, campaign) => Math.max(max, campaignWidth(campaign)), 0) + 3,
    columns - 6,
  ))
  const rule = { text: '  ' + '─'.repeat(innerWidth), hex: getTheme().brandHex }

  const rendered = tickerText(content)
  const total = visibleGraphemeCount(rendered)
  let keep: number
  if (phase === 'erasing') {
    keep = Math.max(0, total - Math.floor((now - state.shownAt) / ERASE_STEP_MS))
  } else {
    keep = typedLength(state.shownAt, now)
  }
  const shown = truncateAnsiToWidth(revealMarkdown(rendered, keep), innerWidth - 1)

  return [{
    lines: [
      line(rule),
      line(plain('  '), ...bandSpans(shown, innerWidth)),
      line(rule),
    ],
    marginTop: 1,
  }]
}
