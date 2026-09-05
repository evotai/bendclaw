/**
 * OutputLine — a single line of REPL output.
 *
 * All REPL output (user messages, assistant text, tool results, verbose events)
 * is modeled as an append-only list of OutputLines. These are rendered by
 * Ink's <Static> component, which writes them once and never re-renders.
 *
 * This module is pure logic — no React, no stdout. Easy to test.
 */

import { renderMarkdown, renderThinkingMarkdown } from './markdown.js'
import { colorizeUnifiedDiffRows, type DiffRowKind } from './diff.js'
import { highlightCode, highlightCodeLine } from '../markdown/render/ansi.js'
import { truncate, formatDuration, formatElapsed, toolResultLines, formatBashCommandDisplay, expandLinesHint, COLLAPSE_HINT, summarizeInline } from './format.js'
import { formatCompactionCompleted } from './verbose.js'
import type { UICompaction, UIMessage, UIToolCall } from '../term/app/types.js'

// ---------------------------------------------------------------------------
// Tool presentation — icon + primary argument per tool, followed by a stable
// lifecycle status row. Optional reasons, progress, diffs, and result output
// always render after those first two rows.
// ---------------------------------------------------------------------------

interface ToolGlyph { icon: string }

/** Map an engine tool name to a compact glyph. Unknown tools fall back to `·`. */
function toolGlyph(name: string): ToolGlyph {
  switch (name.toLowerCase()) {
    case 'bash': return { icon: '⌘' }
    case 'read': case 'read_code': return { icon: '◫' }
    case 'grep': case 'glob': case 'find': case 'search': return { icon: '⌕' }
    case 'web_fetch': case 'webfetch': return { icon: '⊕' }
    case 'edit': case 'file_edit': case 'write': case 'file_write': return { icon: '✎' }
    // Background task tools act on a shell, so they share the bash glyph
    // family rather than falling through to the anonymous `·`.
    case 'task_output': case 'taskoutput': return { icon: '◷' }
    case 'task_stop': case 'taskstop': return { icon: '⊘' }
    default: return { icon: '·' }
  }
}

/** User-facing label for internal tool names. Protocol names remain unchanged. */
function toolDisplayName(name: string): string {
  switch (name.toLowerCase()) {
    case 'task_output': case 'taskoutput': return 'background shell'
    case 'task_stop': case 'taskstop': return 'stop background shell'
    default: return name
  }
}

/** True for the tools that act on an existing background task by id. */
function isTaskTool(name: string): boolean {
  const n = name.toLowerCase()
  return n === 'task_output' || n === 'taskoutput' || n === 'task_stop' || n === 'taskstop'
}

/** Resolve a path-like tool argument, preserving empty/invalid values so failed
 *  cards still show what was attempted (e.g. path="." or a missing path). */
function toolPathArg(args: Record<string, unknown>): { raw: string | undefined; display: string } {
  const raw = args?.path ?? args?.file ?? args?.file_path
  if (typeof raw !== 'string') return { raw: undefined, display: '' }
  const trimmed = raw.trim()
  if (!trimmed) return { raw, display: 'path=""' }
  // Bare "." / "./" is almost always a model mistake for edit/write/read.
  if (trimmed === '.' || trimmed === './') return { raw, display: `path="${trimmed}"` }
  return { raw, display: trimmed }
}

/** Short one-line summary of edit replacements for the failed/expanded card. */
function editReplacementHeadline(args: Record<string, unknown>): string {
  const edits = Array.isArray(args.edits) ? args.edits : []
  if (edits.length === 0) {
    const oldText = typeof args.oldText === 'string' ? args.oldText
      : typeof args.old_string === 'string' ? args.old_string : ''
    const newText = typeof args.newText === 'string' ? args.newText
      : typeof args.new_string === 'string' ? args.new_string : ''
    if (!oldText && !newText) return ''
    return formatReplacementPair(oldText, newText)
  }
  if (edits.length === 1) {
    const edit = edits[0] as Record<string, unknown>
    const oldText = typeof edit?.oldText === 'string' ? edit.oldText
      : typeof edit?.old_string === 'string' ? edit.old_string : ''
    const newText = typeof edit?.newText === 'string' ? edit.newText
      : typeof edit?.new_string === 'string' ? edit.new_string : ''
    return formatReplacementPair(oldText, newText)
  }
  return `${edits.length} replacements`
}

function formatReplacementPair(oldText: string, newText: string): string {
  const oldPart = oldText ? summarizeInline(oldText, 40) : '∅'
  const newPart = newText ? summarizeInline(newText, 40) : '∅'
  return `replace ${JSON.stringify(oldPart)} → ${JSON.stringify(newPart)}`
}

/** The single most useful argument to show beside the tool name. */
function toolPrimaryArg(
  name: string,
  args: Record<string, unknown>,
  previewCommand?: string,
  expanded?: boolean,
  options?: { failed?: boolean },
): string {
  const n = name.toLowerCase()
  if (n === 'bash') {
    const command = previewCommand ?? (args?.command as string) ?? ''
    return formatBashCommandDisplay(command, expanded).headline
  }
  if (isTaskTool(name)) {
    // The engine resolves the task id to its command via preview_command, so
    // concurrent cards name different shells instead of all reading
    // `task_output`. Falls back to the raw id when the task is already gone.
    const command = previewCommand ?? (args?.task_id as string) ?? ''
    if (!command) return ''
    return formatBashCommandDisplay(command, expanded).headline
  }

  const { display: pathDisplay } = toolPathArg(args)
  if (n === 'edit' || n === 'file_edit') {
    const replacement = editReplacementHeadline(args)
    const edits = Array.isArray(args.edits) ? args.edits : []
    // Multi-edit failures list each replacement as detail lines; keep the
    // headline to path only so it doesn't repeat "N replacements".
    if (pathDisplay && options?.failed && edits.length > 1) return pathDisplay
    if (pathDisplay && replacement && options?.failed) {
      return `${pathDisplay} · ${replacement}`
    }
    if (pathDisplay) return pathDisplay
    if (replacement) return replacement
    return options?.failed ? '(missing path)' : ''
  }
  if (n === 'write' || n === 'file_write' || n === 'read' || n === 'read_code') {
    if (pathDisplay) return pathDisplay
    return options?.failed ? '(missing path)' : ''
  }
  if (pathDisplay) return pathDisplay

  const pattern = (args?.pattern ?? args?.query ?? args?.url) as string | undefined
  // Show the full value — the viewmodel wraps the card arg to terminal width,
  // so the tail is never lost. Newlines collapse to a single logical line.
  if (pattern) return String(pattern).replace(/\r?\n/g, ' ').trim()
  return ''
}

/** Tool call line text: `<glyph> <name>  <primary-arg>`. The viewmodel paints
 *  the glyph and parts; status (✓/✗) lives on the subordinate result line. */
function toolCallText(
  name: string,
  args: Record<string, unknown>,
  previewCommand?: string,
  expanded?: boolean,
  options?: { failed?: boolean },
): string {
  const glyph = toolGlyph(name).icon
  const displayName = toolDisplayName(name)
  const primary = toolPrimaryArg(name, args, previewCommand, expanded, options)
  return primary ? `${glyph} ${displayName}  ${primary}` : `${glyph} ${displayName}`
}

function toolDraftText(args: Record<string, unknown>, keys: string[]): string {
  for (const key of keys) {
    const value = args[key]
    if (typeof value === 'string' && value.length > 0) return value
  }
  return ''
}

function lineCount(text: string): number {
  return writePreviewLines(text).length
}

const WRITE_PREVIEW_LINES = 10
const WRITE_STREAM_CONTEXT_LINES = 50
const WRITE_PREVIEW_CACHE_LIMIT = 64
// Above this size, skip syntax highlighting entirely: a synchronous full-file
// highlight of a huge generated file would stall the event loop for seconds.
const WRITE_HIGHLIGHT_MAX_BYTES = 200 * 1024

type WritePreviewCache = {
  path: string
  language: string | undefined
  rawContent: string
  argsComplete: boolean
  /** Content exceeded WRITE_HIGHLIGHT_MAX_BYTES — render unhighlighted. */
  plain: boolean
  displayLines: string[]
  highlightedLines: string[]
}

const writePreviewCache = new Map<string, WritePreviewCache>()

const WRITE_LANGUAGE_BY_EXTENSION: Record<string, string> = {
  bash: 'bash', c: 'c', cc: 'cpp', cpp: 'cpp', cs: 'csharp', css: 'css',
  go: 'go', h: 'c', hpp: 'cpp', html: 'html', java: 'java', js: 'javascript',
  json: 'json', jsonc: 'jsonc', jsx: 'javascript', kt: 'kotlin', lua: 'lua',
  md: 'markdown', mjs: 'javascript', php: 'php', proto: 'proto', py: 'python',
  rb: 'ruby', rs: 'rust', scss: 'scss', sh: 'bash', sql: 'sql', swift: 'swift',
  toml: 'toml', ts: 'typescript', tsx: 'typescript', txt: 'plaintext',
  xml: 'xml', yaml: 'yaml', yml: 'yaml', zsh: 'bash',
}

function writeLanguage(path: string): string | undefined {
  const filename = path.split(/[\\/]/).pop()?.toLowerCase() ?? ''
  if (filename === 'dockerfile') return 'dockerfile'
  if (filename === 'makefile') return 'makefile'
  const dot = filename.lastIndexOf('.')
  return dot >= 0 ? WRITE_LANGUAGE_BY_EXTENSION[filename.slice(dot + 1)] : undefined
}

function normalizeWritePreviewText(text: string): string {
  return text
    .replace(/\r/g, '')
    .replace(/\t/g, '   ')
    // Tool arguments are untrusted model output. Never pass embedded terminal
    // controls through the preview; only line feeds are meaningful here.
    .replace(/[\x00-\x09\x0b-\x1f\x7f]/g, '�')
}

function writePreviewLines(content: string): string[] {
  if (!content) return []
  const lines = normalizeWritePreviewText(content).split('\n')
  while (lines.length > 0 && lines[lines.length - 1] === '') lines.pop()
  return lines
}

function allWritePreviewLines(content: string): string[] {
  return content ? normalizeWritePreviewText(content).split('\n') : []
}

function highlightWriteLines(lines: string[], language: string | undefined): string[] {
  const highlighted = highlightCode(lines.join('\n'), language).split('\n')
  // The highlighter must preserve line structure; if it ever reflows newlines,
  // fall back to per-line highlighting so display/highlight arrays stay in
  // lockstep (trim and slice below index both arrays by the same offsets).
  if (highlighted.length !== lines.length) {
    return lines.map(line => highlightCodeLine(line, language))
  }
  return highlighted
}

function rebuildWritePreview(path: string, content: string, argsComplete: boolean): WritePreviewCache {
  const language = writeLanguage(path)
  const displayLines = allWritePreviewLines(content)
  const plain = content.length > WRITE_HIGHLIGHT_MAX_BYTES
  return {
    path,
    language,
    rawContent: content,
    argsComplete,
    plain,
    displayLines,
    highlightedLines: plain ? [...displayLines] : highlightWriteLines(displayLines, language),
  }
}

function refreshWritePreviewPrefix(cache: WritePreviewCache): void {
  const count = Math.min(WRITE_STREAM_CONTEXT_LINES, cache.displayLines.length)
  if (count === 0) return
  const highlighted = highlightWriteLines(cache.displayLines.slice(0, count), cache.language)
  for (let index = 0; index < count; index++) {
    cache.highlightedLines[index] = highlighted[index]
      ?? highlightCodeLine(cache.displayLines[index] ?? '', cache.language)
  }
}

function cachedWritePreview(call: UIToolCall, path: string, content: string): WritePreviewCache {
  let cache = writePreviewCache.get(call.id)
  if (
    !cache
    || cache.path !== path
    || !content.startsWith(cache.rawContent)
    || (call.argsComplete === true && !cache.argsComplete)
  ) {
    // Final arguments are authoritative: rebuild the complete fragment once so
    // cross-line syntax state is exact before execution begins.
    cache = rebuildWritePreview(path, content, call.argsComplete === true)
  } else if (content.length > cache.rawContent.length) {
    // Highlight the receiving/new lines cheaply, then refresh the first 50 as
    // one source fragment so block comments and multiline strings stay correct.
    // This mirrors pi's bounded streaming strategy.
    const delta = normalizeWritePreviewText(content.slice(cache.rawContent.length))
    cache.rawContent = content
    if (cache.displayLines.length === 0) {
      cache.displayLines.push('')
      cache.highlightedLines.push('')
    }
    const parts = delta.split('\n')
    const last = cache.displayLines.length - 1
    const { plain, language } = cache
    const highlightLine = (line: string): string =>
      plain ? line : highlightCodeLine(line, language)
    cache.displayLines[last] += parts[0] ?? ''
    cache.highlightedLines[last] = highlightLine(cache.displayLines[last]!)
    for (let index = 1; index < parts.length; index++) {
      const line = parts[index] ?? ''
      cache.displayLines.push(line)
      cache.highlightedLines.push(highlightLine(line))
    }
    if (!cache.plain) refreshWritePreviewPrefix(cache)
  }

  writePreviewCache.delete(call.id)
  writePreviewCache.set(call.id, cache)
  while (writePreviewCache.size > WRITE_PREVIEW_CACHE_LIMIT) {
    const oldest = writePreviewCache.keys().next().value
    if (oldest === undefined) break
    writePreviewCache.delete(oldest)
  }
  return cache
}

/**
 * Streamed content preview for a write call: shown while arguments stream
 * (queued) and while the tool is running until the authoritative diff arrives.
 */
function appendWriteContentPreview(lines: OutputLine[], call: UIToolCall, expanded?: boolean): void {
  const name = call.name.toLowerCase()
  if (name !== 'write' && name !== 'file_write') return
  const content = toolDraftText(call.args, ['content'])
  if (!content) return

  const path = toolDraftText(call.args, ['path', 'file', 'file_path'])
  const cache = cachedWritePreview(call, path, content)
  let total = cache.highlightedLines.length
  while (total > 0 && cache.displayLines[total - 1] === '') total--
  if (total === 0) return

  const visibleLines = cache.highlightedLines.slice(0, total)
  const shown = expanded ? visibleLines : visibleLines.slice(0, WRITE_PREVIEW_LINES)
  lines.push({ id: genId('tool-preview-space'), kind: 'tool', text: '' })
  for (const text of shown) {
    lines.push({ id: genId('tool-preview'), kind: 'tool', text: `  ${text}`, toolCodePreview: true })
  }

  const remaining = total - shown.length
  if (remaining > 0) {
    lines.push(toolHintLine(`  ... (${remaining} more ${remaining === 1 ? 'line' : 'lines'}, ${total} total, ctrl+o to expand)`))
  } else if (expanded && total > 1) {
    lines.push(toolHintLine(`  ${COLLAPSE_HINT}`))
  }
}

function toolDraftSummary(call: UIToolCall): string {
  const name = call.name.toLowerCase()
  if (name === 'write' || name === 'file_write') {
    const content = toolDraftText(call.args, ['content'])
    const count = content ? lineCount(content) : 0
    return count > 0 ? `generating ${count} ${count === 1 ? 'line' : 'lines'}` : 'generating content'
  }
  if (name === 'edit' || name === 'file_edit') {
    const edits = Array.isArray(call.args.edits) ? call.args.edits.length : 0
    return edits > 0 ? `preparing ${edits} ${edits === 1 ? 'replacement' : 'replacements'}` : 'preparing replacement'
  }
  return 'preparing arguments'
}

function toolStatusLine(mark: '○' | '●' | '✓' | '✗', parts: string[]): OutputLine {
  const detail = parts.filter(Boolean).join(' · ')
  return {
    id: genId('tool-status'),
    kind: 'tool',
    text: `  ${mark}${detail ? ` · ${detail}` : ''}`,
  }
}

/** A collapsed/expand hint row under a card. */
function toolHintLine(text: string): OutputLine {
  return { id: genId('tool-hint'), kind: 'tool_result', text }
}

function insertToolStatus(lines: OutputLine[], status: OutputLine): void {
  const callIndex = lines.findIndex(line => line.kind === 'tool' && !line.text.startsWith('  '))
  lines.splice(callIndex < 0 ? 0 : callIndex + 1, 0, status)
}

/** One OutputLine per diff row, so the viewmodel can tint added/removed rows. */
function diffOutputLines(diff: string): OutputLine[] {
  return colorizeUnifiedDiffRows(diff).map(row => ({
    id: genId('tool-diff'),
    kind: 'tool' as const,
    text: row.text,
    diffRow: row.kind,
  }))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface OutputLine {
  id: string
  kind: 'user' | 'assistant' | 'thinking' | 'tool' | 'tool_result' | 'verbose' | 'error' | 'system' | 'cancelled'
  text: string
  rawMarkdown?: string
  /** Thinking text already contains markdown ANSI; apply only the outer tint. */
  thinkingStyle?: boolean
  /** System text that already carries its own ANSI styling (e.g. `/skill`).
   *  Rendered verbatim instead of being flattened to one dim gray. */
  preStyled?: boolean
  /** Tool line containing pre-styled source code from a streamed write call. */
  toolCodePreview?: boolean
  /** Tool line that is one row of a rendered diff; added/removed rows get
   *  their own fill inside the card. */
  diffRow?: DiffRowKind
  codeBlockId?: string
  codeLanguage?: string
  /** Visual spacer inserted between streamed markdown chunks. It creates a
   *  blank line but must not start a new assistant message marker. */
  isContinuationSpacer?: boolean
  /** First line of a committed user/assistant message: gets an OSC 133 zone
   *  start marker so terminals can select/copy the whole message. */
  zoneStart?: boolean
  /** Last line of a committed user/assistant message: gets the OSC 133 zone
   *  end marker. */
  zoneEnd?: boolean
  /** Wall-clock time the message was committed, in epoch millis. Captured at
   *  build time (not render time) so a re-render — and the incremental history
   *  cache — reproduces the same header a full rebuild would. */
  timestamp?: number
  /**
   * Membership in a tool card. Every line of one call carries the same state
   * so the viewmodel can lay the whole card on the panel; `first`/`last` mark
   * the card's edges, where the padded rows go.
   */
  toolCard?: ToolCardMembership
}

export type ToolCardState = 'pending' | 'success' | 'error'

export interface ToolCardMembership {
  state: ToolCardState
  first: boolean
  last: boolean
}

/** Stamp a finished card's lines with their shared state and edges. */
function stampToolCard(lines: OutputLine[], state: ToolCardState): OutputLine[] {
  const last = lines.length - 1
  return lines.map((line, index) => ({
    ...line,
    toolCard: { state, first: index === 0, last: index === last },
  }))
}

// ---------------------------------------------------------------------------
// ID generator
// ---------------------------------------------------------------------------

let nextId = 0

function genId(prefix: string): string {
  return `${prefix}-${nextId++}`
}

/** Reset ID counter (for tests). */
export function resetIdCounter(): void {
  nextId = 0
}

// ---------------------------------------------------------------------------
// Builders — pure functions that create OutputLines from events
// ---------------------------------------------------------------------------

export function buildUserMessage(text: string, timestamp: number = Date.now()): OutputLine[] {
  if (!text) return []
  return [{ id: genId('user'), kind: 'user', text, zoneStart: true, zoneEnd: true, timestamp }]
}

export function buildAssistantLines(
  markdownText: string,
  options: { streaming?: boolean } = {},
): OutputLine[] {
  if (!markdownText.trim()) return []
  const rendered = renderMarkdown(markdownText, { streaming: options.streaming })
  if (!rendered || !rendered.trim()) return []
  const cleaned = rendered.replace(/^\n+/, '').replace(/\n+$/, '')
  const parts = cleaned.split('\n')
  return parts.map((line, i) => ({
    id: genId('asst'),
    kind: 'assistant' as const,
    text: line,
    rawMarkdown: markdownText,
    // Wrap the whole assistant message in one OSC 133 zone (first line starts,
    // last line ends) so it selects/copies as a single block.
    zoneStart: i === 0,
    zoneEnd: i === parts.length - 1,
  }))
}

/**
 * True when reasoning text carries something to read.
 *
 * An evot-pro-anthropic diagnostic request returned a thinking_delta whose
 * entire text was `...`, after its tool call. This placeholder is already in
 * the gateway's wire response; it is not evidence of a short reasoning turn
 * or of any particular OpenAI summary policy. Suppress it only in the view,
 * retaining the original signed thinking block for replay. Real text that
 * merely contains dots ("checking ... now") must still render.
 */
function hasReasoningContent(text: string): boolean {
  return text.replace(/[.\u2026\s]/g, '').length > 0
}

/**
 * Reasoning rows, rendered as markdown and tinted muted by the viewmodel.
 *
 * Shown in full: reasoning is the most useful thing on screen while a long
 * turn runs, so it is never collapsed behind a summary row. `streaming` only
 * affects fence handling in the markdown pipeline.
 */
export function buildThinkingLines(
  text: string,
  options: { streaming?: boolean } = {},
): OutputLine[] {
  if (!text.trim() || !hasReasoningContent(text)) return []
  const rendered = renderThinkingMarkdown(text, { streaming: options.streaming })
  if (!rendered || !rendered.trim()) return []
  const cleaned = rendered.replace(/^\n+/, '').replace(/\n+$/, '')
  return cleaned.split('\n').map((line) => ({
    id: genId('think'),
    kind: 'thinking' as const,
    text: line,
    thinkingStyle: true,
  }))
}

export function buildToolCall(
  name: string,
  args: Record<string, unknown>,
  previewCommand?: string,
  expanded?: boolean,
  options?: { failed?: boolean },
): OutputLine[] {
  // Keep the operation headline first so every lifecycle state has the same
  // stable card geometry: headline → status → optional reason/output details.
  const lines: OutputLine[] = [{
    id: genId('tool'),
    kind: 'tool',
    text: toolCallText(name, args, previewCommand, expanded, options),
  }]
  for (const reason of formatReasonLines(args)) {
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${reason}` })
  }
  // Failed edit cards: show each replacement summary under the headline so the
  // user can see what was attempted without expanding the full result body.
  if (options?.failed) {
    for (const detail of failedArgDetailLines(name, args)) {
      lines.push({ id: genId('tool'), kind: 'tool', text: `  ${detail}` })
    }
  }
  // Expanded multi-line bash: keep newlines instead of flattening into a wall.
  // Collapse hint matches expanded tool results / progress cards — but only
  // when the user explicitly expanded (not auto-expanded failures).
  if (name.toLowerCase() === 'bash' && expanded) {
    const command = previewCommand ?? (args?.command as string) ?? ''
    const details = formatBashCommandDisplay(command, true).detailLines
    for (const detail of details) {
      lines.push({ id: genId('tool'), kind: 'tool', text: detail })
    }
    if (details.length > 0) {
      lines.push(toolHintLine(`  [2m${COLLAPSE_HINT}[0m`))
    }
  }
  return lines
}

/** Extra indented lines that only appear on failed cards to make the attempt
 *  obvious (multi-edit replacements, empty path, etc.). */
function failedArgDetailLines(name: string, args: Record<string, unknown>): string[] {
  const n = name.toLowerCase()
  if (n !== 'edit' && n !== 'file_edit') return []
  const edits = Array.isArray(args.edits) ? args.edits : []
  if (edits.length <= 1) return []
  const lines: string[] = []
  const maxShow = 3
  for (let i = 0; i < Math.min(edits.length, maxShow); i++) {
    const edit = edits[i] as Record<string, unknown>
    const oldText = typeof edit?.oldText === 'string' ? edit.oldText
      : typeof edit?.old_string === 'string' ? edit.old_string : ''
    const newText = typeof edit?.newText === 'string' ? edit.newText
      : typeof edit?.new_string === 'string' ? edit.new_string : ''
    lines.push(`${i + 1}/${edits.length} ${formatReplacementPair(oldText, newText)}`)
  }
  if (edits.length > maxShow) {
    lines.push(`… +${edits.length - maxShow} more replacements`)
  }
  return lines
}

export function buildToolCard(call: UIToolCall, expanded?: boolean, _now = Date.now()): OutputLine[] {
  // ask_user owns an interactive overlay and commits the selected answer (or
  // cancellation) separately. Rendering its engine-side lifecycle as a generic
  // tool card duplicates that UI with an unhelpful `ready/running` card.
  if (isAskUserTool(call.name)) return []

  const details = asDetails(call.details)
  const diff = typeof details.diff === 'string' ? details.diff : undefined
  const args = diff ? { ...call.args, diff } : call.args
  // Pre-compute failure so the headline can surface path="." / missing path and
  // edit replacement summaries before the status line is inserted.
  const exitCode = call.name.toLowerCase() === 'bash' ? detailNumber(details, 'exit_code') : undefined
  const settledFailed = call.status === 'error'
    || details.error === true
    || (exitCode !== undefined && exitCode !== 0)
  // Note: failure enriches the headline ({failed}) but does NOT force the
  // command body open — a huge heredoc stays collapsed; ctrl+o still works.
  const lines = buildToolCall(
    call.name,
    args,
    call.previewCommand,
    expanded,
    { failed: settledFailed },
  )

  if (call.status === 'queued') {
    const summary = call.argsComplete ? 'ready' : toolDraftSummary(call)
    insertToolStatus(lines, toolStatusLine('○', [summary]))
    appendWriteContentPreview(lines, call, expanded)
    return stampToolCard(lines, 'pending')
  }

  if (call.status !== 'running') {
    // The call has settled; its streaming preview cache is no longer needed.
    writePreviewCache.delete(call.id)
    // buildToolResult auto-previews failed bodies itself (tail lines), so pass
    // the raw user toggle: ctrl+o still expands/collapses failed cards.
    const resultLines = buildToolResult(
      call.name,
      args,
      call.status,
      call.result,
      call.durationMs,
      expanded,
      details,
    )
    insertToolStatus(lines, resultLines.shift() ?? toolStatusLine(settledFailed ? '✗' : '✓', []))
    lines.push(...resultLines)
    return stampToolCard(lines, settledFailed ? 'error' : 'success')
  }

  insertToolStatus(lines, toolStatusLine('●', runningStatusParts(call.name, args)))
  if (diff) {
    lines.push(...diffOutputLines(diff))
  } else {
    // Keep the streamed content visible between tool_started and the engine's
    // preview diff so the card never blanks out mid-transition (pi behavior).
    appendWriteContentPreview(lines, call, expanded)
  }
  if (call.progress && !call.progress.startsWith('__evot_spill_event__ ')) {
    const progressLines = toolResultLines(formatToolResultContent(call.progress), false, call.name, expanded)
    for (const text of progressLines) {
      lines.push({ id: genId('tool-progress'), kind: 'tool_result', text: `  ${text}` })
    }
  }

  return stampToolCard(lines, 'pending')
}

/** Failed tool bodies auto-preview at most this many tail lines. */
const ERROR_PREVIEW_LINES = 20

/**
 * Status parts for a card that is still executing.
 *
 * Most tools just say `running`. A `task_output` says what it is doing to the
 * task instead, because several such cards can be in flight at once and
 * `running` on each one says nothing about which is which or why it is waiting.
 */
function runningStatusParts(name: string, args: Record<string, unknown>): string[] {
  const n = name.toLowerCase()
  if (n === 'task_output' || n === 'taskoutput') {
    // `block` defaults to true, matching the engine's `is_none_or`.
    const blocking = args?.block !== false
    return [blocking ? 'waiting for task' : 'checking task']
  }
  if (n === 'task_stop' || n === 'taskstop') return ['stopping task']
  return ['running']
}

export function buildToolResult(
  name: string,
  args: Record<string, unknown>,
  status: 'done' | 'error',
  result?: string,
  durationMs?: number,
  expanded?: boolean,
  details: Record<string, unknown> = {},
): OutputLine[] {
  const exitCode = name.toLowerCase() === 'bash' ? detailNumber(details, 'exit_code') : undefined
  const isError = status === 'error' || details.error === true || (exitCode !== undefined && exitCode !== 0)
  const summary = toolResultSummary(name, args, result, details, isError)
  const duration = durationMs !== undefined ? formatDuration(durationMs) : ''
  // Failed status: lead with "failed" so the line reads as an outcome, not a
  // size/duration metric ("74 B · 0ms" is meaningless for a missing path).
  const backgrounded = details.backgrounded === true
  // A task tool reports the *task's* outcome, not the poll's: `completed · exit
  // 0 · 1m 47s` rather than a byte count of the response body.
  const taskParts = !isError && isTaskTool(name) ? taskToolStatusParts(details) : []
  // Glyph decisions read the status field, not the rendered label. They used to
  // string-match the label ("still running", "failed"), which meant rewording a
  // card silently changed its icon: renaming the backgrounded label to "running
  // in background" flipped a live task from `●` to `✓`, reporting unfinished work
  // as done.
  const taskStatus = !isError && isTaskTool(name) ? detailString(details, 'status') : undefined
  // The poll succeeded but the task did not. Marking that `✓` would report a
  // broken build as a success, so the glyph follows the task, not the call.
  const taskFailed = taskStatus === 'failed'
  // A poll that found the task still running is in-flight work, not a completed
  // step, so it keeps the `●` running glyph — whether the task is detached or
  // still being waited on in the foreground.
  const stillRunning = taskStatus === 'running' || taskStatus === 'running_foreground'
  const statusParts = isError
    ? ['failed', summary, duration].filter(Boolean)
    : taskParts.length > 0
      ? taskParts
      : [summary, duration].filter(Boolean)
  const mark = isError || taskFailed ? '✗' : backgrounded || stillRunning ? '●' : '✓'
  const lines: OutputLine[] = [toolStatusLine(mark, statusParts)]

  // Diff (for write/edit tools) — skip on failure; the attempt is already
  // summarized on the headline and the error body is what matters.
  const diff = args?.diff as string | undefined
  if (!isError && diff && typeof diff === 'string' && diff.length > 0) {
    lines.push(...diffOutputLines(diff))
  }

  // Tool result content. Collapsed success is a single `... (+N lines, ctrl+o
  // to expand)` hint. Failures auto-preview the tail of the body (errors live
  // at the end) capped at ERROR_PREVIEW_LINES; ctrl+o expands the rest and
  // collapses back to the preview.
  if (result) {
    const formattedResult = formatToolResultContent(result)
    if (isError && !expanded) {
      const all = toolResultLines(formattedResult, true, name, true)
      const hidden = all.length - ERROR_PREVIEW_LINES
      if (hidden > 0) {
        lines.push(toolHintLine(`  [2m... ${expandLinesHint(hidden)}[0m`))
      }
      for (const rl of hidden > 0 ? all.slice(-ERROR_PREVIEW_LINES) : all) {
        lines.push({ id: genId('tool-res'), kind: 'error', text: `  ${rl}` })
      }
    } else {
      const resultLines = toolResultLines(formattedResult, isError, name, expanded)
      for (const rl of resultLines) {
        lines.push({ id: genId('tool-res'), kind: isError ? 'error' : 'tool_result', text: `  ${rl}` })
      }
      // Collapse hint under any user-expanded multiline body (success or
      // failure) — ctrl+o collapses back to the default view.
      if (expanded && resultLines.length > 1) {
        lines.push(toolHintLine(`  [2m${COLLAPSE_HINT}[0m`))
      }
    }
  }

  return lines
}

export function buildToolProgress(name: string, text: string, expanded?: boolean): OutputLine[] {
  const progressLines = text.replace(/\r\n/g, '\n').replace(/\n+$/, '').split('\n')
  const total = progressLines.length
  const header = `${toolGlyph(name).icon} ${name}  · ${total} ${total === 1 ? 'line' : 'lines'}`
  const lines: OutputLine[] = [{ id: genId('tool'), kind: 'tool', text: header }]
  if (expanded) {
    // Expanded: full progress body + collapse hint.
    for (const l of progressLines) {
      lines.push({ id: genId('tool-res'), kind: 'tool_result', text: `  ${l}` })
    }
    if (progressLines.length > 1) {
      lines.push(toolHintLine(`  [2m${COLLAPSE_HINT}[0m`))
    }
    return lines
  }
  // Collapsed: no content preview — the header already carries the line count,
  // so a multiline body just adds a single expand hint (matching tool results).
  if (total > 1) {
    lines.push(toolHintLine(`  [2m... ${expandLinesHint(total)}[0m`))
  } else {
    lines.push({ id: genId('tool-res'), kind: 'tool_result', text: `  ${progressLines[0] ?? ''}` })
  }
  return lines
}

export function buildVerboseEvent(eventText: string): OutputLine[] {
  if (!eventText) return []
  return eventText.split('\n').map((line) => ({
    id: genId('verb'),
    kind: 'verbose' as const,
    text: line,
  }))
}

/** True for events that must always reach the TUI: LLM errors/retries,
 *  significant prompt-cache misses, and compactions that actually changed
 *  context. Per-call/no-op stats remain in screen.log only. */
export function isVisibleEvent(text: string): boolean {
  return /^\[LLM\]\s+[↻✗⚠]/u.test(text)
    || /^\[COMPACT\]\s+✓\s+·\s+(?!skipped\b)/u.test(text)
}

/**
 * Render a visible LLM event (error / retry / cache-miss notice) as a
 * tool-style card so it reads like any other tool in the stream:
 *   ✦ llm  <model|retry|cache miss …>
 *     ✗|↻|⚠ · <meta>
 *     <error message>
 * Falls back to plain verbose lines if the text isn't in the expected shape.
 */
export function buildLlmCard(text: string): OutputLine[] {
  const rawLines = text.split('\n')
  const head = (rawLines[0] ?? '').match(/^\[LLM\]\s+([↻✗⚠])\s*·?\s*(.*)$/u)
  if (!head) return buildVerboseEvent(text)
  const mark = head[1]!
  const rest = (head[2] ?? '').trim()
  const isRetry = mark === '↻'
  // Body: drop the `    error     ` label, keep the message text.
  const body = rawLines.slice(1)
    .map((l) => l.replace(/^\s*error\s+/u, '').trim())
    .filter((l) => l.length > 0)

  const lines: OutputLine[] = []
  if (isRetry) {
    lines.push({ id: genId('tool'), kind: 'tool', text: '✦ llm  retry' })
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${mark} · ${rest}` })
  } else {
    const parts = rest.split(' · ')
    const model = parts[0] ?? 'unknown'
    const meta = parts.slice(1).join(' · ')
    lines.push({ id: genId('tool'), kind: 'tool', text: `✦ llm  ${model}` })
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${mark}${meta ? ` · ${meta}` : ''}` })
  }
  for (const b of body) {
    lines.push({ id: genId('tool-res'), kind: 'error', text: `  ${b}` })
  }
  return lines
}

export function buildEventCard(text: string): OutputLine[] {
  if (text.startsWith('[COMPACT]')) {
    const rawLines = text.split('\n')
    const head = (rawLines[0] ?? '').replace(/^\[COMPACT\]\s+✓\s*·?\s*/u, '').trim()
    const lines: OutputLine[] = [
      { id: genId('tool'), kind: 'tool', text: '✦ compact' },
      { id: genId('tool'), kind: 'tool', text: `  ✓ · ${head}` },
    ]
    for (const line of rawLines.slice(1)) {
      const body = line.replace(/^\s*(context|summary)\s*/u, '').trim()
      if (body) lines.push({ id: genId('tool-res'), kind: 'tool_result', text: `  ${body}` })
    }
    return lines
  }
  return buildLlmCard(text)
}

export function buildError(message: string): OutputLine[] {
  return [{ id: genId('err'), kind: 'error', text: `Error: ${message}` }]
}

export function buildSystem(text: string): OutputLine[] {
  return [{ id: genId('sys'), kind: 'system', text }]
}

// ---------------------------------------------------------------------------
// Convert UIMessages to OutputLines (for resume)
// ---------------------------------------------------------------------------

function buildCompactionLines(compaction: UICompaction, expanded: boolean): OutputLine[] {
  const messagesEvicted = Math.max(0, compaction.messagesBefore - compaction.messagesAfter)
  const details = formatCompactionCompleted({
    reason: compaction.reason,
    result: {
      type: 'compacted',
      before_message_count: compaction.messagesBefore,
      after_message_count: compaction.messagesAfter,
      before_tokens: compaction.tokensBefore,
      after_tokens: compaction.tokensAfter,
      messages_evicted: messagesEvicted,
      current_run_reclaimed: 0,
      method: compaction.method,
      remote_blob_bytes: compaction.remoteBlobBytes,
      fallback_reason: compaction.fallbackReason,
    },
  })
  const lines = buildEventCard(details)
  if (!compaction.summary.trim()) return lines
  if (expanded) return [...lines, ...buildAssistantLines(compaction.summary)]
  return [
    ...lines,
    toolHintLine('  ... summary hidden (ctrl+o to expand)'),
  ]
}

/** The `    error     <msg>` tail an LLM event card carries, if any. */
export function errorTailOf(text: string): string | null {
  const m = text.match(/\n\s*error\s+([\s\S]+)$/u)
  return m ? m[1]!.trim() : null
}

/**
 * Drop an event's error tail when the card above already printed that sentence.
 *
 * A failed attempt and the retry that follows it carry the same provider
 * message, so the pair said one thing twice. The retry line keeps the part only
 * it knows — the countdown and the attempt counter.
 */
export function withoutRedundantErrorTail(text: string, known: string | null): string {
  if (!known) return text
  const tail = errorTailOf(text)
  if (tail === null || tail !== known) return text
  return text.replace(/\n\s*error\s+[\s\S]+$/u, '')
}

export function messagesToOutputLines(messages: UIMessage[], expanded: boolean = false): OutputLine[] {
  const lines: OutputLine[] = []
  for (const msg of messages) {
    if (msg.compaction) {
      lines.push(...buildCompactionLines(msg.compaction, expanded))
      continue
    }
    if (msg.role === 'user') {
      lines.push(...buildUserMessage(msg.text, msg.timestamp))
      continue
    }

    // Replay the same always-visible event cards as the live stream — which
    // means collapsing a retry storm the same way. The live path drops the
    // repeats before they reach scrollback; a stored transcript still holds
    // every attempt, so resuming a session would bring the wall of duplicate
    // 529s back. Rendering is the only thing filtered here: the events stay in
    // the transcript, so no stored data changes meaning.
    if (msg.verboseEvents) {
      let retryCardShown = false
      let lastErrorShown: string | null = null
      for (const evt of msg.verboseEvents) {
        const isRetry = evt.kind === 'llm_retry'
        const isFailedCall = evt.kind === 'llm_completed' && /^\[LLM\]\s+✗/u.test(evt.text)
        if (isRetry || isFailedCall) {
          if (retryCardShown) continue
          if (isRetry) retryCardShown = true
        } else {
          // Anything else — a successful call, a compaction — ends the storm,
          // so a later independent failure is announced rather than swallowed.
          retryCardShown = false
          lastErrorShown = null
        }
        if (!isVisibleEvent(evt.text)) continue
        const text = withoutRedundantErrorTail(evt.text, lastErrorShown)
        lines.push(...buildEventCard(text))
        lastErrorShown = errorTailOf(evt.text) ?? lastErrorShown
      }
    }

    if (msg.content) {
      for (const block of [...msg.content].sort((a, b) => a.contentIndex - b.contentIndex)) {
        if (block.type === 'thinking') {
          lines.push(...buildThinkingLines(block.text))
        }
        else if (block.type === 'text') lines.push(...buildAssistantLines(block.text))
        else lines.push(...buildToolCard(block.toolCall))
      }
    } else if (msg.text.trim()) {
      // Legacy UI messages created before ordered assistant content existed.
      lines.push(...buildAssistantLines(msg.text))
    }
  }
  return lines
}

// ---------------------------------------------------------------------------
// Code-block-aware split (inspired by qwen-code's markdownUtilities)
// ---------------------------------------------------------------------------

/**
 * Check if a character index falls inside an unclosed fenced code block.
 */
function isInsideCodeBlock(content: string, index: number): boolean {
  let fenceCount = 0
  let pos = 0
  while (pos < content.length) {
    const next = content.indexOf('```', pos)
    if (next === -1 || next >= index) break
    fenceCount++
    pos = next + 3
  }
  return fenceCount % 2 === 1
}

/**
 * Find the last safe split point in `content` — a position where we can
 * cut without breaking a code block.  Prefers `\n\n` (paragraph boundary),
 * falls back to `\n`.  Returns `content.length` when no safe split exists.
 */
export function findSafeSplitPoint(content: string): number {
  // If the tail is inside an unclosed code block, don't split at all.
  if (isInsideCodeBlock(content, content.length)) return content.length

  // Prefer paragraph boundary (\n\n) not inside a code block.
  let search = content.length
  while (search >= 0) {
    const idx = content.lastIndexOf('\n\n', search)
    if (idx === -1) break
    const splitAt = idx + 2
    if (!isInsideCodeBlock(content, splitAt)) return splitAt
    search = idx - 1
  }

  // Fall back to last single newline not inside a code block.
  const nlPos = content.lastIndexOf('\n')
  if (nlPos > 0 && !isInsideCodeBlock(content, nlPos + 1)) return nlPos + 1

  return content.length
}

// ---------------------------------------------------------------------------
// AssistantStreamBuffer — accumulates streaming tokens, emits lines
// ---------------------------------------------------------------------------

export class AssistantStreamBuffer {
  private buffer = ''
  private started = false

  /** Push a token. Returns OutputLines to append (may be empty). */
  push(token: string): OutputLine[] {
    if (!token) return []
    this.buffer += token

    if (!this.started) {
      this.buffer = this.buffer.replace(/^[\n\r]+/, '')
      if (this.buffer.length === 0) return []
      this.started = true
    }

    return this.flushSafe()
  }

  /** Flush remaining buffer. Returns OutputLines to append. */
  finish(): OutputLine[] {
    if (!this.started) return []
    const result: OutputLine[] = []
    if (this.buffer.trim().length > 0) {
      result.push(...buildAssistantLines(this.buffer))
    }
    this.buffer = ''
    this.started = false
    return result
  }

  /** The current incomplete text (for display in dynamic zone). */
  get pendingText(): string {
    return this.started ? this.buffer : ''
  }

  get isStarted(): boolean {
    return this.started
  }

  /**
   * Flush completed content using code-block-aware splitting.
   * Only the portion before the safe split point is rendered and emitted;
   * the rest stays in the buffer for the dynamic zone.
   */
  private flushSafe(): OutputLine[] {
    if (!this.buffer.includes('\n')) return []

    const splitAt = findSafeSplitPoint(this.buffer)
    if (splitAt === this.buffer.length || splitAt === 0) return []

    const completeText = this.buffer.slice(0, splitAt)
    this.buffer = this.buffer.slice(splitAt)

    return buildAssistantLines(completeText)
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function isAskUserTool(name: string): boolean {
  const normalized = name.toLowerCase().replace(/_/g, '')
  return normalized === 'askuser'
}

function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

function parseJsonResult(content: string): unknown | undefined {
  const trimmed = content.trim()
  if (!trimmed) return undefined
  const first = trimmed[0]
  if (first !== '{' && first !== '[') return undefined
  try {
    return JSON.parse(trimmed)
  } catch {
    return undefined
  }
}

function asDetails(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {}
}

function detailNumber(details: Record<string, unknown>, key: string): number | undefined {
  const value = details[key]
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

function detailString(details: Record<string, unknown>, key: string): string | undefined {
  const value = details[key]
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

/**
 * Status parts for a settled `task_output` / `task_stop` card.
 *
 * `● · running` alone was unreadable with several tasks in flight: it named
 * neither the task, its runtime, nor whether the poll actually returned
 * anything. The outcome leads, because that is what the next step depends on.
 */
function taskToolStatusParts(details: Record<string, unknown>): string[] {
  const status = detailString(details, 'status')
  const retrieval = detailString(details, 'retrieval_status')
  const exitCode = detailNumber(details, 'exit_code')
  const elapsedMs = detailNumber(details, 'elapsed_ms')
  const lines = detailNumber(details, 'total_lines')
  const parts: string[] = []

  // Outcome first. A still-running task says so plainly rather than reporting
  // the poll as a success.
  switch (status) {
    case 'completed':
      parts.push(exitCode !== undefined ? `completed · exit ${exitCode}` : 'completed')
      break
    case 'failed':
      parts.push(exitCode !== undefined ? `failed · exit ${exitCode}` : 'failed')
      break
    case 'killed':
      // "cancelled" rather than "stopped" when it was the user's decision: the
      // work is void, not merely halted.
      parts.push(details.stopped_by_user === true ? 'cancelled by user' : 'stopped')
      break
    // `running` is `RunningBackground`; `running_foreground` is a shell still
    // being waited on. Both used to render as a bare "still running", which hid
    // the distinction that matters most to a user reading the card: whether the
    // work is now detached and safe to leave, or still holding a wait. The bash
    // card has always named the background; this one now agrees with it.
    case 'running':
      parts.push(runningInBackgroundLabel(retrieval))
      break
    case 'running_foreground':
      // No "in background" here: this task really is still in the foreground.
      // The `timeout` clause is legacy for the same reason as above: replayed
      // transcripts still carry it, nothing new writes it.
      parts.push(retrieval === 'timeout' ? 'still running · wait timed out' : 'still running')
      break
    default:
      if (status) parts.push(status)
  }

  if (elapsedMs !== undefined) parts.push(formatElapsed(elapsedMs))
  if (lines !== undefined && lines > 0) parts.push(plural(lines, 'line'))
  return parts
}

/**
 * How a backgrounded task reads on a `task_output` card.
 *
 * "running in background" rather than a bare "still running", matching the bash
 * card and Claude Code: the useful fact is that the work is detached and the
 * turn is free, not merely that it has not finished.
 *
 * Only a hit deadline earns an extra clause. `released` and `not_ready` both
 * mean the same thing to someone reading the card — the task is detached and
 * this poll returned no result — and "running in background · stopped waiting"
 * said that twice.
 */
function runningInBackgroundLabel(retrieval: string | undefined): string {
  // `timeout` is a legacy value: waits are no longer cut short by a bound, so
  // nothing writes it any more. Tool-result details are persisted per
  // transcript, so sessions recorded before that change still replay through
  // here and must not render a bare, unexplained "running in background".
  return retrieval === 'timeout'
    ? 'running in background · wait timed out'
    : 'running in background'
}

function resultLineCount(content: string): number {
  const normalized = content.replace(/\r\n|\r/g, '\n').replace(/\n+$/, '')
  return normalized ? normalized.split('\n').length : 0
}

function plural(count: number, singular: string, pluralForm = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : pluralForm}`
}

function readLineSummary(content: string): string | undefined {
  const firstLine = content.replace(/\r\n|\r/g, '\n').split('\n', 1)[0] ?? ''
  const full = /^\[(\d+) lines?\]$/.exec(firstLine)
  if (full) return plural(Number(full[1]), 'line')
  const range = /^\[Lines \d+-\d+ of (\d+)\]$/.exec(firstLine)
  return range ? plural(Number(range[1]), 'line') : undefined
}

function resultProtocolLines(content: string): string[] {
  return content.replace(/\r\n|\r/g, '\n').replace(/\n+$/, '').split('\n')
}

function isIncompleteResultLine(line: string): boolean {
  return /^\.\.\. \((?:capped|search timed out|truncated)\b/.test(line)
}

function grepResultSummary(args: Record<string, unknown>, result: string): string | undefined {
  const lines = resultProtocolLines(result)
  const filesOnly = args.files_with_matches === true
  const noMatches = lines.length === 1 && /^\(no matches(?:;|\))/.test(lines[0] ?? '')
  if (noMatches) {
    const summary = filesOnly ? '0 files' : '0 matches'
    return `${summary}${lines[0]!.includes(';') ? ' shown' : ''}`
  }

  const incomplete = lines.some(isIncompleteResultLine)
  const contentLines = lines.filter(line => !isIncompleteResultLine(line))
  if (filesOnly) {
    if (contentLines.some(line => line === '--' || line.length === 0)) return undefined
    const count = contentLines.length
    return `${plural(count, 'file')}${incomplete ? ' shown' : ''}`
  }

  let matches = 0
  for (const line of contentLines) {
    if (line === '--') continue
    if (/^.+:\d+: /.test(line)) {
      matches++
      continue
    }
    if (/^.+-\d+- /.test(line)) continue
    return undefined
  }
  return `${plural(matches, 'match', 'matches')}${incomplete ? ' shown' : ''}`
}

function globResultSummary(args: Record<string, unknown>, result: string): string | undefined {
  const lines = resultProtocolLines(result)
  const type = typeof args.type === 'string' ? args.type : 'f'
  const noun = type === 'd' ? 'directory' : type === 'any' ? 'path' : 'file'
  const noMatches = lines.length === 1 && /^\(no matches(?:;|\))/.test(lines[0] ?? '')
  if (noMatches) {
    const summary = plural(0, noun, type === 'd' ? 'directories' : `${noun}s`)
    return `${summary}${lines[0]!.includes(';') ? ' shown' : ''}`
  }

  const incomplete = lines.some(isIncompleteResultLine)
  const contentLines = lines.filter(line => !isIncompleteResultLine(line))
  if (contentLines.some(line => line.length === 0)) return undefined
  const count = contentLines.length
  const summary = plural(count, noun, type === 'd' ? 'directories' : `${noun}s`)
  return `${summary}${incomplete ? ' shown' : ''}`
}

/**
 * Headline for a command that is in the background, naming *why*.
 *
 * "started" only fits the case where backgrounding was the plan. A command that
 * ran for its whole deadline, or that was moved aside so the user could talk,
 * did not merely start -- reading it that way hides both the wait that already
 * happened and the fact that the user's own keypress caused it.
 *
 * Unknown reasons fall back to the neutral wording: this string is written by
 * the engine, so an older or newer one must not render as an empty status.
 */
function backgroundedSummary(details: Record<string, unknown>): string {
  switch (detailString(details, 'background_reason')) {
    case 'timeout_elapsed':
      return 'still running · moved to background at its deadline'
    case 'yield_elapsed':
      return 'still running · moved to background'
    case 'user_requested':
      return 'still running · backgrounded by you'
    case 'message_delivery':
      return 'still running · backgrounded to read your message'
    default:
      return 'started · running in background'
  }
}

function toolResultSummary(
  name: string,
  args: Record<string, unknown>,
  result: string | undefined,
  details: Record<string, unknown>,
  isError = false,
): string {
  const normalizedName = name.toLowerCase()
  const bytes = detailNumber(details, 'bytes')
  const lines = result ? resultLineCount(result) : 0

  if (normalizedName === 'bash') {
    if (details.backgrounded === true) return backgroundedSummary(details)
    const exitCode = detailNumber(details, 'exit_code')
    if (exitCode !== undefined) return `exit ${exitCode}`
  }

  // Failures: status line stays short ("failed · exit N · 12ms"). The full
  // error body is auto-expanded below, so do not echo it again here.
  if (isError) {
    if (normalizedName === 'web_fetch' || normalizedName === 'webfetch') {
      const status = detailNumber(details, 'status')
      if (status !== undefined) return `HTTP ${status}`
    }
    if (normalizedName === 'edit' || normalizedName === 'file_edit'
      || normalizedName === 'write' || normalizedName === 'file_write'
      || normalizedName === 'read' || normalizedName === 'read_code') {
      const { display } = toolPathArg(args)
      if (!display || display.startsWith('path=')) return 'invalid path'
    }
    return ''
  }

  if (normalizedName === 'read' || normalizedName === 'read_code') {
    if (bytes !== undefined) return humanBytes(bytes)
    if (result) return readLineSummary(result) ?? plural(lines, 'line')
  }

  if (normalizedName === 'write' || normalizedName === 'file_write') {
    if (bytes !== undefined) return `${details.created === true ? 'created' : 'wrote'} ${humanBytes(bytes)}`
  }

  if (normalizedName === 'edit' || normalizedName === 'file_edit') {
    const replacements = detailNumber(details, 'replacement_count')
    const added = detailNumber(details, 'added_lines')
    const removed = detailNumber(details, 'removed_lines')
    const parts: string[] = []
    if (replacements !== undefined) parts.push(plural(replacements, 'replacement'))
    if (added !== undefined || removed !== undefined) parts.push(`+${added ?? 0} −${removed ?? 0}`)
    if (parts.length > 0) return parts.join(' · ')
  }

  if (normalizedName === 'search') {
    const hits = detailNumber(details, 'hits')
    if (hits !== undefined) return plural(hits, 'hit')
  }

  if (normalizedName === 'grep' && result) {
    const summary = grepResultSummary(args, result)
    if (summary) return summary
  }

  if (normalizedName === 'glob' && result) {
    const summary = globResultSummary(args, result)
    if (summary) return summary
  }

  if (normalizedName === 'web_fetch' || normalizedName === 'webfetch') {
    const status = detailNumber(details, 'status')
    if (status !== undefined) return `HTTP ${status}${lines > 1 ? ` · ${plural(lines, 'line')}` : ''}`
  }

  if (lines > 1) {
    return result
      ? `${plural(lines, 'line')} · ${humanBytes(Buffer.byteLength(result, 'utf-8'))}`
      : plural(lines, 'line')
  }
  return result ? humanBytes(Buffer.byteLength(result, 'utf-8')) : ''
}

function formatToolResultContent(content: string): string {
  const parsed = parseJsonResult(content)
  if (parsed === undefined) return content
  return JSON.stringify(parsed, null, 2)
}

/** Reason-style fields the model fills to justify a call. Rendered separately
 *  as ↳ lines and excluded from the generic arg list. */
function isReasonKey(key: string): boolean {
  return key === 'reason' || key.startsWith('reason_to_')
}

/** Human label for a reason field key. */
function reasonLabel(key: string): string {
  switch (key) {
    case 'reason':
      return 'reason'
    case 'reason_to_increase_timeout':
      return 'why longer timeout'
    case 'reason_to_use_instead_of_read_file_tool':
      return 'why not read'
    case 'reason_to_use_instead_of_edit_file_tool':
      return 'why not edit'
    case 'reason_to_use_instead_of_glob_files_tool':
      return 'why not glob'
    default:
      return key.replace(/^reason_to_/, 'why ').replace(/_/g, ' ')
  }
}

/** Build ↳ lines for any reason fields present, skipping empty or 'N/A'. */
function formatReasonLines(args: Record<string, unknown>): string[] {
  if (!args || typeof args !== 'object') return []
  const lines: string[] = []
  for (const [k, v] of Object.entries(args)) {
    if (!isReasonKey(k)) continue
    if (typeof v !== 'string') continue
    const val = v.trim()
    if (val === '' || val === 'N/A') continue
    lines.push(`↳ ${reasonLabel(k)}: ${truncate(val, 120)}`)
  }
  return lines
}
