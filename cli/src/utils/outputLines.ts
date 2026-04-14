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

export function buildAssistantPrefix(): OutputLine[] {
  return [{ id: genId('apfx'), kind: 'assistant' as const, text: '⏺' }]
}

export function buildAssistantLines(markdownText: string): OutputLine[] {
  if (!markdownText.trim()) return []
  const rendered = renderMarkdown(markdownText)
  if (!rendered || !rendered.trim()) return []
  const cleaned = rendered.replace(/^\n+/, '').replace(/\n+$/, '')
  return cleaned.split('\n').map((line) => ({
    id: genId('asst'),
    kind: 'assistant' as const,
    text: line,
  }))
}

export function buildToolCall(
  name: string,
  args: Record<string, unknown>,
  previewCommand?: string,
): OutputLine[] {
  const lines: OutputLine[] = []
  // Badge line: [tool_name] call
  lines.push({
    id: genId('tool'),
    kind: 'tool',
    text: `[${name.toUpperCase()}] call`,
  })
  // Detail: preview command takes priority, otherwise show args
  if (previewCommand) {
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ❯ ${previewCommand}` })
  } else {
    for (const line of formatToolInputLines(args)) {
      lines.push({ id: genId('tool'), kind: 'tool', text: `  ${line}` })
    }
  }
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

  const dur = durationMs !== undefined ? ` · ${durationMs}ms` : ''
  const badge = name.toUpperCase()
  const label = status === 'error' ? `[${badge}] failed${dur}` : `[${badge}] completed${dur}`
  lines.push({
    id: genId('tool'),
    kind: 'tool',
    text: label,
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
  const line = (text: string) => lines.push({ id: genId('summary'), kind: 'run_summary' as const, text })

  // Header
  line('─── This Run Summary ──────────────────────────────────')
  line(`${dur} · ${stats.turnCount} ${pl(stats.turnCount, 'turn')} · ${stats.llmCalls} llm ${pl(stats.llmCalls, 'call')} · ${stats.toolCallCount} tool ${pl(stats.toolCallCount, 'call')} · ${humanTokens(totalTokens)} tokens`)

  // Context budget bar (only if meaningful)
  if (stats.contextWindow > 0 && stats.contextTokens > 0) {
    const budget = stats.contextWindow
    const pct = (stats.contextTokens / budget) * 100
    if (pct > 0) {
      const bar = renderBar(stats.contextTokens, budget, 20)
      line(`  context   ${bar}  ${pct.toFixed(0)}%(${humanTokens(stats.contextTokens)}) of budget(${humanTokens(budget)})`)
    }
  }

  // Tokens
  const totalStreamMs = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0)
  const overallTps = totalStreamMs > 0 ? (stats.outputTokens / (totalStreamMs / 1000)).toFixed(1) : '0'
  let tokLine = `  tokens    ${humanTokens(stats.inputTokens)} input · ${stats.outputTokens} output · ${overallTps} tok/s`
  if (stats.cacheReadTokens > 0 || stats.cacheWriteTokens > 0) {
    const hitRate = stats.inputTokens > 0
      ? (stats.cacheReadTokens / stats.inputTokens * 100).toFixed(0)
      : '0'
    tokLine += ` · cache ${hitRate}%`
  }
  line(tokLine)

  // LLM call details
  if (stats.llmCallDetails.length > 0) {
    const totalLlmMs = stats.llmCallDetails.reduce((s, c) => s + c.durationMs, 0)
    const llmPct = stats.durationMs > 0 ? (totalLlmMs / stats.durationMs * 100).toFixed(1) : '0'
    const avgTps = (stats.llmCallDetails.reduce((s, c) => s + c.tokPerSec, 0) / stats.llmCallDetails.length).toFixed(1)
    line(`  llm       ${stats.llmCallDetails.length} ${pl(stats.llmCallDetails.length, 'call')} · ${formatDuration(totalLlmMs)} (${llmPct}% of run) · ${avgTps} tok/s avg`)

    const avgTtft = stats.llmCallDetails.reduce((s, c) => s + c.ttftMs, 0) / stats.llmCallDetails.length
    const avgStream = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0) / stats.llmCallDetails.length
    line(`            ttft avg ${formatDuration(Math.round(avgTtft))} · stream avg ${formatDuration(Math.round(avgStream))}`)

    // All calls by duration
    const sorted = [...stats.llmCallDetails].sort((a, b) => b.durationMs - a.durationMs)
    const show = Math.min(sorted.length, 3)
    const maxDur = sorted[0]?.durationMs ?? 1
    for (let i = 0; i < show; i++) {
      const c = sorted[i]!
      const bar = renderBar(c.durationMs, maxDur, 20)
      const pct = totalLlmMs > 0 ? (c.durationMs / totalLlmMs * 100).toFixed(1) : '0'
      line(`            #${i + 1}  ${formatDuration(c.durationMs)} ${bar} ${pct}%`)
    }
    if (sorted.length > 3) {
      const restMs = sorted.slice(3).reduce((s, c) => s + c.durationMs, 0)
      line(`            ... ${sorted.length - 3} more · ${formatDuration(restMs)} total`)
    }
  }

  // Tool breakdown (compact, under llm section)
  if (stats.toolBreakdown.length > 0) {
    for (const tb of stats.toolBreakdown) {
      const errStr = tb.errors > 0 ? ` · ${tb.errors} err` : ''
      line(`              ${tb.name}  ${tb.count} ${pl(tb.count, 'call')}  ${formatDuration(tb.totalDurationMs)}${errStr}`)
    }
  }

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
        lines.push(...buildAssistantPrefix())
        lines.push(...buildAssistantLines(msg.text))
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
    const result: OutputLine[] = []
    if (!this.prefixEmitted) {
      result.push(...buildAssistantPrefix())
      this.prefixEmitted = true
    }
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

/** Format all args as key: value lines (matching Rust REPL's format_tool_input_lines). */
function formatToolInputLines(args: Record<string, unknown>): string[] {
  if (!args || typeof args !== 'object') return []
  const entries = Object.entries(args).filter(([k]) => k !== 'diff')
  if (entries.length === 0) return []
  return entries.map(([k, v]) => {
    let val: string
    if (typeof v === 'string') val = truncate(v, 120)
    else if (Array.isArray(v)) val = truncate(v.map(String).join(', '), 120)
    else val = truncate(String(v), 120)
    return `${k}: ${val}`
  })
}
