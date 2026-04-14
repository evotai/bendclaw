/**
 * OutputLine — a single line of REPL output.
 *
 * All REPL output (user messages, assistant text, tool results, verbose events)
 * is modeled as an append-only list of OutputLines. These are rendered by
 * Ink's <Static> component, which writes them once and never re-renders.
 *
 * This module is pure logic — no React, no stdout. Easy to test.
 */

import { renderMarkdown } from './markdown.js'
import { colorizeUnifiedDiff } from './diff.js'
import { truncate, truncateResult, humanTokens, formatDuration, renderBar } from './format.js'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface OutputLine {
  id: string
  kind: 'user' | 'assistant' | 'tool' | 'verbose' | 'error' | 'system' | 'run_summary'
  text: string
  /** ANSI-styled text ready for display. If absent, `text` is used. */
  styled?: string
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

export function buildUserMessage(text: string): OutputLine[] {
  return [{ id: genId('user'), kind: 'user', text }]
}

export function buildAssistantLines(markdownText: string, withPrefix = false): OutputLine[] {
  if (!markdownText.trim()) return []
  const rendered = renderMarkdown(markdownText)
  if (!rendered || !rendered.trim()) return []
  const cleaned = rendered.replace(/^\n+/, '').replace(/\n+$/, '')
  const lines = cleaned.split('\n')
  return lines.map((line, i) => ({
    id: genId('asst'),
    kind: 'assistant' as const,
    text: (i === 0 && withPrefix) ? `⏺ ${line}` : line,
  }))
}

export function buildToolCall(
  name: string,
  args: Record<string, unknown>,
  previewCommand?: string,
): OutputLine[] {
  const lines: OutputLine[] = []
  const detail = previewCommand || formatToolDetail(args)
  lines.push({
    id: genId('tool'),
    kind: 'tool',
    text: `⚙ ${name}${detail ? ` ${detail}` : ''}`,
  })
  return lines
}

export function buildToolResult(
  name: string,
  args: Record<string, unknown>,
  status: 'done' | 'error',
  result?: string,
  durationMs?: number,
): OutputLine[] {
  const lines: OutputLine[] = []

  const icon = status === 'error' ? '✗' : '✓'
  const detail = formatToolDetail(args)
  const dur = durationMs !== undefined ? ` (${durationMs}ms)` : ''
  lines.push({
    id: genId('tool'),
    kind: 'tool',
    text: `${icon} ${name}${detail ? ` ${detail}` : ''}${dur}`,
    styled: undefined, // styled in the React component
  })

  // Diff
  const diff = args?.diff as string | undefined
  if (diff && typeof diff === 'string' && diff.length > 0) {
    lines.push({
      id: genId('tool-diff'),
      kind: 'tool',
      text: colorizeUnifiedDiff(diff),
    })
  }

  // Error preview
  if (status === 'error' && result) {
    lines.push({
      id: genId('tool-err'),
      kind: 'error',
      text: truncateResult(result, 200),
    })
  }

  return lines
}

export function buildVerboseEvent(eventText: string): OutputLine[] {
  const lines = eventText.split('\n').map((line) => ({
    id: genId('verb'),
    kind: 'verbose' as const,
    text: line,
  }))
  // Add empty separator line after each verbose block (matches Rust REPL style)
  lines.push({ id: genId('verb'), kind: 'verbose' as const, text: '' })
  return lines
}

export function buildRunSummary(stats: import('../state/AppState.js').RunStats): OutputLine[] {
  const lines: OutputLine[] = []
  const dur = formatDuration(stats.durationMs)
  const totalTokens = stats.inputTokens + stats.outputTokens
  const pl = (n: number, s: string) => n === 1 ? s : `${s}s`

  // Header
  lines.push({ id: genId('summary'), kind: 'run_summary', text: '─── Run Summary ──────────────────────────────────' })
  lines.push({
    id: genId('summary'), kind: 'run_summary',
    text: `${dur} · ${stats.turnCount} ${pl(stats.turnCount, 'turn')} · ${stats.llmCalls} llm · ${stats.toolCallCount} ${pl(stats.toolCallCount, 'tool')} · ${totalTokens} tokens`,
  })

  // Context budget bar (only if meaningful)
  if (stats.contextWindow > 0 && stats.contextTokens > 0) {
    const budget = stats.contextWindow
    const pct = ((stats.contextTokens / budget) * 100).toFixed(0)
    if (Number(pct) > 0) {
      const bar = renderBar(stats.contextTokens, budget, 20)
      lines.push({
        id: genId('summary'), kind: 'run_summary',
        text: `  context   ${bar}  ${pct}%(${humanTokens(stats.contextTokens)}) of budget(${humanTokens(budget)})`,
      })
    }
  }

  // Tokens
  let tokLine = `  tokens    ${humanTokens(stats.inputTokens)} in · ${stats.outputTokens} out`
  if (stats.cacheReadTokens > 0 || stats.cacheWriteTokens > 0) {
    const hitRate = stats.inputTokens > 0
      ? (stats.cacheReadTokens / stats.inputTokens * 100).toFixed(0)
      : '0'
    tokLine += ` · cache ${hitRate}%`
  }
  lines.push({ id: genId('summary'), kind: 'run_summary', text: tokLine })

  // Tool breakdown
  if (stats.toolBreakdown.length > 0) {
    lines.push({ id: genId('summary'), kind: 'run_summary', text: '  tools' })
    for (const tb of stats.toolBreakdown) {
      const errStr = tb.errors > 0 ? ` · ${tb.errors} err` : ''
      lines.push({
        id: genId('summary'), kind: 'run_summary',
        text: `            ${tb.name.padEnd(20)} ${tb.count}× · ${formatDuration(tb.totalDurationMs)}${errStr}`,
      })
    }
  }

  // LLM call details
  if (stats.llmCallDetails.length > 0) {
    const totalLlmMs = stats.llmCallDetails.reduce((s, c) => s + c.durationMs, 0)
    const llmPct = stats.durationMs > 0 ? (totalLlmMs / stats.durationMs * 100).toFixed(0) : '0'
    const avgTps = stats.llmCallDetails.length > 0
      ? (stats.llmCallDetails.reduce((s, c) => s + c.tokPerSec, 0) / stats.llmCallDetails.length).toFixed(1)
      : '0'
    lines.push({
      id: genId('summary'), kind: 'run_summary',
      text: `  llm       ${stats.llmCallDetails.length} ${pl(stats.llmCallDetails.length, 'call')} · ${formatDuration(totalLlmMs)} (${llmPct}% of run) · ${avgTps} tok/s`,
    })

    const avgTtft = stats.llmCallDetails.reduce((s, c) => s + c.ttftMs, 0) / stats.llmCallDetails.length
    const avgStream = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0) / stats.llmCallDetails.length
    lines.push({
      id: genId('summary'), kind: 'run_summary',
      text: `            ttft avg ${formatDuration(Math.round(avgTtft))} · stream avg ${formatDuration(Math.round(avgStream))}`,
    })

    // Top 3 by duration
    const sorted = [...stats.llmCallDetails].sort((a, b) => b.durationMs - a.durationMs)
    const show = Math.min(sorted.length, 3)
    const maxDur = sorted[0]?.durationMs ?? 1
    for (let i = 0; i < show; i++) {
      const c = sorted[i]!
      const bar = renderBar(c.durationMs, maxDur, 20)
      const pct = totalLlmMs > 0 ? (c.durationMs / totalLlmMs * 100).toFixed(0) : '0'
      lines.push({
        id: genId('summary'), kind: 'run_summary',
        text: `            #${i + 1}  ${formatDuration(c.durationMs).padEnd(6)} ${bar} ${pct}%`,
      })
    }
    if (sorted.length > 3) {
      const restMs = sorted.slice(3).reduce((s, c) => s + c.durationMs, 0)
      lines.push({
        id: genId('summary'), kind: 'run_summary',
        text: `            ... ${sorted.length - 3} more · ${formatDuration(restMs)} total`,
      })
    }
  }

  // Footer
  lines.push({ id: genId('summary'), kind: 'run_summary', text: '──────────────────────────────────────────────────' })

  return lines
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

export function messagesToOutputLines(messages: import('../state/AppState.js').UIMessage[]): OutputLine[] {
  const lines: OutputLine[] = []
  for (const msg of messages) {
    if (msg.role === 'user') {
      lines.push(...buildUserMessage(msg.text))
    } else if (msg.role === 'assistant') {
      // Tool calls first
      if (msg.toolCalls) {
        for (const tc of msg.toolCalls) {
          lines.push(...buildToolResult(
            tc.name,
            tc.args,
            tc.status === 'error' ? 'error' : 'done',
            tc.result,
            tc.durationMs,
          ))
        }
      }
      // Assistant text
      if (msg.text.trim()) {
        lines.push(...buildAssistantLines(msg.text, true))
      }
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
  private prefixEmitted = false

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
    const needsPrefix = !this.prefixEmitted
    const lines = this.buffer.trim().length > 0
      ? buildAssistantLines(this.buffer, needsPrefix)
      : []
    if (needsPrefix && lines.length > 0) this.prefixEmitted = true
    this.buffer = ''
    this.started = false
    return lines
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

function formatToolDetail(args: Record<string, unknown>): string {
  if (!args || typeof args !== 'object') return ''
  if ('command' in args) return truncate(String(args.command), 80)
  if ('path' in args) return truncate(String(args.path), 80)
  if ('file_path' in args) return truncate(String(args.file_path), 80)
  if ('pattern' in args) return truncate(String(args.pattern), 60)
  if ('url' in args) return truncate(String(args.url), 80)
  return ''
}
