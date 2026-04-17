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
import { truncate, truncateResult, humanTokens, formatDuration, renderBar, toolResultLines } from './format.js'
import type { RunStats, UIMessage } from '../state/types.js'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface OutputLine {
  id: string
  kind: 'user' | 'assistant' | 'tool' | 'tool_result' | 'verbose' | 'error' | 'system' | 'run_summary'
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

export function buildUserMessage(text: string, imageCount?: number): OutputLine[] {
  const lines: OutputLine[] = [{ id: genId('user'), kind: 'user', text }]
  if (imageCount && imageCount > 0) {
    lines.push({
      id: genId('user-img'),
      kind: 'user',
      text: `  [${imageCount} image${imageCount > 1 ? 's' : ''} attached]`,
    })
  }
  return lines
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
  // Detail: preview command takes priority (shown in full), otherwise show args
  if (previewCommand) {
    const cmdLines = previewCommand.split('\n')
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ❯ ${cmdLines[0]}` })
    for (let i = 1; i < cmdLines.length; i++) {
      lines.push({ id: genId('tool'), kind: 'tool', text: `    ${cmdLines[i]}` })
    }
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
  const isError = status === 'error'

  const dur = durationMs !== undefined ? ` · ${durationMs}ms` : ''
  const badge = name.toUpperCase()
  const label = isError ? `[${badge}] failed${dur}` : `[${badge}] completed${dur}`
  lines.push({
    id: genId('tool'),
    kind: 'tool',
    text: label,
  })

  // Diff (for write/edit tools)
  const diff = args?.diff as string | undefined
  if (diff && typeof diff === 'string' && diff.length > 0) {
    lines.push({
      id: genId('tool-diff'),
      kind: 'tool',
      text: colorizeUnifiedDiff(diff),
    })
  }

  // Tool result content (head/tail truncated)
  if (result) {
    const resultLines = toolResultLines(result, isError)
    for (const rl of resultLines) {
      lines.push({
        id: genId('tool-res'),
        kind: isError ? 'error' : 'tool_result',
        text: `  ${rl}`,
      })
    }
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

export function buildRunSummary(stats: RunStats): OutputLine[] {
  const lines: OutputLine[] = []
  const dur = formatDuration(stats.durationMs)
  const totalTokens = stats.inputTokens + stats.outputTokens
  const pl = (n: number, s: string) => n === 1 ? s : `${s}s`
  const line = (text: string) => lines.push({ id: genId('summary'), kind: 'run_summary' as const, text })
  const barWidth = 20

  // Header
  line('─── This Run Summary ──────────────────────────────────')
  line(`${dur} · ${stats.turnCount} ${pl(stats.turnCount, 'turn')} · ${stats.llmCalls} llm ${pl(stats.llmCalls, 'call')} · ${stats.toolCallCount} tool ${pl(stats.toolCallCount, 'call')} · ${humanTokens(totalTokens)} tokens`)

  // Context budget bar
  if (stats.contextWindow > 0 && stats.contextTokens > 0) {
    const budget = stats.contextWindow
    const pct = (stats.contextTokens / budget) * 100
    if (pct > 0) {
      const bar = renderBar(stats.contextTokens, budget, barWidth)
      line(`  context   ${bar}  ${pct.toFixed(0)}%(${humanTokens(stats.contextTokens)}) of budget(${humanTokens(budget)})`)
    }
  }
  line('')

  // --- tokens block ---
  const totalInput = stats.inputTokens
  const totalStreamMs = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0)
  const overallTps = totalStreamMs > 0 ? (stats.outputTokens / (totalStreamMs / 1000)).toFixed(1) : '0'
  let tokLine = `  tokens    ${humanTokens(totalInput)} total input · ${stats.outputTokens} output · ${overallTps} tok/s`
  if (stats.cacheReadTokens > 0 || stats.cacheWriteTokens > 0) {
    const hitRate = totalInput > 0
      ? (stats.cacheReadTokens / totalInput * 100).toFixed(0)
      : '0'
    tokLine += ` · cache ${hitRate}%`
  }
  line(tokLine)

  // Token breakdown by role
  const ms = stats.lastMessageStats
  if (ms && totalInput > 0) {
    const sysTok = stats.systemPromptTokens
    const maxLabelWidth = 12
    const maxValWidth = 8
    const roles: [string, number][] = [
      ['system', sysTok],
      ['user', ms.userTokens],
      ['assistant', ms.assistantTokens],
      ['tool_result', ms.toolResultTokens],
    ]
    for (const [label, tokens] of roles) {
      if (tokens === 0) continue
      const pct = tokens / totalInput * 100
      const bar = renderBar(tokens, totalInput, barWidth)
      line(`            ${label.padEnd(maxLabelWidth)} ${('~' + humanTokens(tokens)).padStart(maxValWidth)}  ${bar} ${pct.toFixed(1).padStart(5)}%`)
    }

    // Per-tool breakdown under tool_result
    if (ms.toolDetails.length >= 2) {
      // Aggregate by tool name
      const agg = new Map<string, { calls: number; tokens: number }>()
      for (const [name, tokens] of ms.toolDetails) {
        const existing = agg.get(name)
        if (existing) {
          existing.calls++
          existing.tokens += tokens
        } else {
          agg.set(name, { calls: 1, tokens })
        }
      }
      const sorted = [...agg.entries()].sort((a, b) => b[1].tokens - a[1].tokens)
      const subIndent = 18
      const callWord = (n: number) => n === 1 ? 'call' : 'calls'
      const maxLeft = Math.max(...sorted.map(([name, a]) =>
        `${name}  ${a.calls} ${callWord(a.calls)}  ~${humanTokens(a.tokens)}`.length
      ))
      for (const [name, a] of sorted) {
        const pct = totalInput > 0 ? (a.tokens / totalInput * 100).toFixed(1) : '0'
        const bar = renderBar(a.tokens, totalInput, barWidth)
        const left = `${name}  ${a.calls} ${callWord(a.calls)}  ~${humanTokens(a.tokens)}`
        line(`${' '.repeat(subIndent)}${left.padEnd(maxLeft + 2)}${bar} ${pct.padStart(5)}%`)
      }
    }
  }
  line('')

  // --- compact block ---
  if (stats.compactHistory.length > 0) {
    const totalSaved = stats.compactHistory.reduce((s, c) => s + (c.beforeTokens - c.afterTokens), 0)
    line(`  compact   ${stats.compactHistory.length} ${pl(stats.compactHistory.length, 'compaction')} · saved ${humanTokens(totalSaved)} tokens`)

    const runOnce = stats.compactHistory.filter(c => c.level === 0)
    const real = stats.compactHistory.filter(c => c.level > 0)

    for (const c of runOnce) {
      const saved = c.beforeTokens - c.afterTokens
      line(`            run-once  ${humanTokens(c.beforeTokens)}→${humanTokens(c.afterTokens)}  saved ${humanTokens(saved)}`)
    }
    for (let i = 0; i < real.length; i++) {
      const c = real[i]!
      const saved = c.beforeTokens - c.afterTokens
      const pct = c.beforeTokens > 0 ? (saved / c.beforeTokens * 100).toFixed(0) : '0'
      const bar = renderBar(saved, c.beforeTokens || 1, 12)
      line(`            #${i + 1}  lv${c.level}  ${humanTokens(c.beforeTokens)}→${humanTokens(c.afterTokens)}  saved ${humanTokens(saved)}  ${bar} ${pct}%`)
    }
    line('')
  }

  // --- llm block ---
  if (stats.llmCallDetails.length > 0) {
    const totalLlmMs = stats.llmCallDetails.reduce((s, c) => s + c.durationMs, 0)
    const llmPct = stats.durationMs > 0 ? (totalLlmMs / stats.durationMs * 100).toFixed(1) : '0'
    const totalOutputTok = stats.llmCallDetails.reduce((s, c) => s + c.outputTokens, 0)
    const totalLlmStreamMs = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0)
    const avgTps = totalLlmStreamMs > 0 ? (totalOutputTok / (totalLlmStreamMs / 1000)).toFixed(1) : '0'
    line(`  llm       ${stats.llmCallDetails.length} ${pl(stats.llmCallDetails.length, 'call')} · ${formatDuration(totalLlmMs)} (${llmPct}% of run) · ${avgTps} tok/s avg`)

    const avgTtft = stats.llmCallDetails.reduce((s, c) => s + c.ttftMs, 0) / stats.llmCallDetails.length
    const avgStream = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0) / stats.llmCallDetails.length
    line(`            ttft avg ${formatDuration(Math.round(avgTtft))} · stream avg ${formatDuration(Math.round(avgStream))}`)

    // Top 3 LLM calls by duration (right-aligned)
    const sorted = [...stats.llmCallDetails].sort((a, b) => b.durationMs - a.durationMs)
    const show = Math.min(sorted.length, 3)
    const maxDur = sorted[0]?.durationMs ?? 1
    const maxIdxWidth = Math.max(...sorted.slice(0, show).map((_, i) => `#${i + 1}`.length))
    const maxDurWidth = Math.max(...sorted.slice(0, show).map(c => formatDuration(c.durationMs).length))
    for (let i = 0; i < show; i++) {
      const c = sorted[i]!
      const bar = renderBar(c.durationMs, maxDur, barWidth)
      const pct = totalLlmMs > 0 ? (c.durationMs / totalLlmMs * 100).toFixed(1) : '0'
      const idx = `#${i + 1}`.padEnd(maxIdxWidth)
      const durStr = formatDuration(c.durationMs).padStart(maxDurWidth)
      line(`            ${idx}  ${durStr} ${bar} ${pct.padStart(5)}%`)
    }
    if (sorted.length > 3) {
      const restMs = sorted.slice(3).reduce((s, c) => s + c.durationMs, 0)
      line(`            ... ${sorted.length - 3} more · ${formatDuration(restMs)} total`)
    }
  }

  // Footer
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

export function messagesToOutputLines(messages: UIMessage[]): OutputLine[] {
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
