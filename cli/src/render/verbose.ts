/**
 * Shared verbose event text formatters.
 *
 * Used by both reducer.ts (real-time streaming) and transcript.ts (history replay)
 * to produce identical output from stats event data.
 */
import { formatDuration, humanTokens, renderBar, renderPositionBar } from './format.js'

// ---------------------------------------------------------------------------
// LLM call started
// ---------------------------------------------------------------------------

export function formatLlmCallStarted(data: Record<string, unknown>): string {
  const model = (data.model as string) ?? '?'
  const turn = (data.turn as number) ?? 0
  const attempt = (data.attempt as number) ?? 0
  const msgCount = (data.message_count as number) ?? 0
  const injectedCount = (data.injected_count as number) ?? 0
  const sysTok = (data.system_prompt_tokens as number) ?? 0
  const retryStr = attempt > 0 ? ` · retry ${attempt}` : ''
  const injectedStr = injectedCount > 0 ? ` · ${injectedCount} injected` : ''

  // Optional message_stats from Rust (always present in real-time, may be absent in transcript)
  const ms = data.message_stats as Record<string, any> | undefined

  const detailLines: string[] = []

  // Messages breakdown — merged into header as parenthetical
  let msgBreakdown = ''
  if (ms) {
    const parts: string[] = []
    if (ms.user_count > 0) parts.push(`user ${ms.user_count}`)
    if (ms.assistant_count > 0) parts.push(`asst ${ms.assistant_count}`)
    if (ms.tool_result_count > 0) parts.push(`tool ${ms.tool_result_count}`)
    if ((ms.image_count as number) > 0) parts.push(`img ${ms.image_count}`)
    if (parts.length > 0) msgBreakdown = ` (${parts.join(' · ')})`
  }

  // Context window bar
  const contextWindow = (data.context_window as number) ?? 0
  const estimatedContextTokens = (data.estimated_context_tokens as number) ?? 0
  if (contextWindow > 0) {
    const total = estimatedContextTokens > 0
      ? estimatedContextTokens
      : ms
        ? sysTok + (ms.user_tokens ?? 0) + (ms.assistant_tokens ?? 0) + (ms.tool_result_tokens ?? 0) + (ms.image_tokens ?? 0)
        : 0
    if (total > 0) {
      const pct = ((total / contextWindow) * 100).toFixed(0)
      const bar = renderBar(total, contextWindow, 20)
      detailLines.push(`  ctx     ${bar}  ~${humanTokens(total)} / ${humanTokens(contextWindow)} · ${pct}%`)
    }
  }

  // Token distribution by role
  if (ms) {
    const dist: string[] = []
    if (sysTok > 0) dist.push(`sys ${humanTokens(sysTok)}`)
    if ((ms.user_tokens as number) > 0) dist.push(`user ${humanTokens(ms.user_tokens)}`)
    if ((ms.assistant_tokens as number) > 0) dist.push(`asst ${humanTokens(ms.assistant_tokens)}`)
    if ((ms.tool_result_tokens as number) > 0) dist.push(`tool ${humanTokens(ms.tool_result_tokens)}`)
    if ((ms.image_tokens as number) > 0) dist.push(`img ${humanTokens(ms.image_tokens)}`)
    if (dist.length > 0) detailLines.push(`  tok     ${dist.join(' · ')}`)
  } else {
    const bytes = (data.message_bytes as number) ?? 0
    const kb = bytes >= 1024 ? `${(bytes / 1024).toFixed(0)} KB` : `${bytes} B`
    detailLines.push(`  tok     ${msgCount} msgs · ${kb} · system ${humanTokens(sysTok)}`)
  }

  // Per-tool token breakdown (top 3 + count when >= 4 tools, full list when 2-3)
  if (ms) {
    const rawDetails = ms.tool_details as [string, number][] | undefined
    if (rawDetails && rawDetails.length >= 2) {
      const agg = new Map<string, number>()
      for (const [name, tokens] of rawDetails) {
        agg.set(name, (agg.get(name) ?? 0) + tokens)
      }
      const sorted = [...agg.entries()].sort((a, b) => b[1] - a[1])
      const total = (ms.tool_result_tokens as number) || 1
      const maxNameLen = Math.max(...sorted.map(([n]) => n.length), 4)

      const TOP = 3
      if (sorted.length <= TOP + 2) {
        // Show all when manageable
        for (const [name, tokens] of sorted) {
          const pct = ((tokens / total) * 100).toFixed(0)
          detailLines.push(`          ${name.padEnd(maxNameLen)}  ~${humanTokens(tokens).padEnd(6)} ${pct.padStart(3)}%`)
        }
      } else {
        // Top N + omitted count
        for (const [name, tokens] of sorted.slice(0, TOP)) {
          const pct = ((tokens / total) * 100).toFixed(0)
          detailLines.push(`          ${name.padEnd(maxNameLen)}  ~${humanTokens(tokens).padEnd(6)} ${pct.padStart(3)}%`)
        }
        const omitted = sorted.length - TOP
        const omittedTokens = sorted.slice(TOP).reduce((s, [, t]) => s + t, 0)
        const omittedPct = ((omittedTokens / total) * 100).toFixed(0)
        detailLines.push(`          ... ${omitted} more tools  ~${humanTokens(omittedTokens).padEnd(6)} ${omittedPct.padStart(3)}%`)
      }
    }
  }

  const turnStr = turn != null ? ` · turn ${turn}` : ''
  return `[LLM] ● ${model}${turnStr} · ${msgCount} msgs${msgBreakdown}${retryStr}${injectedStr}\n${detailLines.join('\n')}`
}

// ---------------------------------------------------------------------------
// LLM call completed
// ---------------------------------------------------------------------------

export function formatLlmCallCompleted(data: Record<string, unknown>): string {
  const model = data.model as string | undefined
  const turn = data.turn as number | undefined
  const error = data.error as string | undefined
  const usage = data.usage as Record<string, number> | undefined
  const metrics = data.metrics as Record<string, number> | undefined
  const durationMs = (data.duration_ms as number) ?? metrics?.duration_ms ?? 0

  if (error) {
    return `[LLM] ✗ ${model ?? 'unknown'}${turn != null ? ` · turn ${turn}` : ''} · ${formatDuration(durationMs)}\n  ${error}`
  }

  const inputTok = usage?.input ?? (data.input_tokens as number) ?? 0
  const outputTok = usage?.output ?? (data.output_tokens as number) ?? 0
  const tokPerSec = durationMs > 0 ? (outputTok / (durationMs / 1000)).toFixed(0) : '0'
  const ttfbMs = (data.time_to_first_byte_ms as number) ?? metrics?.ttfb_ms ?? 0
  const streamingMs = metrics?.streaming_ms ?? Math.max(0, durationMs - ttfbMs)
  const dur = durationMs || 1
  const ttfbPct = ((ttfbMs / dur) * 100).toFixed(0)
  const streamPct = ((streamingMs / dur) * 100).toFixed(0)

  const lines: string[] = []
  lines.push(`[LLM] ✓ ${model ?? 'unknown'}${turn != null ? ` · turn ${turn}` : ''} · ${formatDuration(durationMs)} · ${tokPerSec} tok/s`)
  lines.push(`  tok     ${humanTokens(inputTok)} in · ${humanTokens(outputTok)} out`)
  lines.push(`  timing  ttfb ${(ttfbMs / 1000).toFixed(1)}s · ${ttfbPct}% · stream ${(streamingMs / 1000).toFixed(1)}s · ${streamPct}%`)
  return lines.join('\n')
}

// ---------------------------------------------------------------------------
// Context compaction started
// ---------------------------------------------------------------------------

export function formatCompactionStarted(data: Record<string, unknown>): string {
  const msgCount = ((data.message_count as number) ?? (data.messages_count as number)) ?? 0
  const estTokens = (data.estimated_tokens as number) ?? 0
  const contextWindow = (data.context_window as number) ?? 0
  const sysTok = (data.system_prompt_tokens as number) ?? 0
  const pct = contextWindow > 0 ? ((estTokens / contextWindow) * 100).toFixed(0) : '0'
  const bar = renderBar(estTokens, contextWindow, 20)

  const detailLines: string[] = []
  if (contextWindow > 0) detailLines.push(`  ctx     ${bar}  ~${humanTokens(estTokens)} / ${humanTokens(contextWindow)} · ${pct}%`)

  // Token distribution if available
  const cms = (data.message_stats as Record<string, any> | undefined) ?? (data.token_breakdown as Record<string, any> | undefined)
  if (cms) {
    const parts: string[] = []
    const uTok = ((cms.user_tokens as number) ?? (cms.user as number)) ?? 0
    const aTok = ((cms.assistant_tokens as number) ?? (cms.assistant as number)) ?? 0
    const trTok = ((cms.tool_result_tokens as number) ?? (cms.tool as number)) ?? 0
    const imgTok = ((cms.image_tokens as number) ?? (cms.image as number)) ?? 0
    const effectiveSysTok = sysTok || ((cms.system_tokens as number) ?? (cms.system as number) ?? 0)
    if (effectiveSysTok > 0) parts.push(`sys ${humanTokens(effectiveSysTok)}`)
    if (uTok > 0) parts.push(`user ${humanTokens(uTok)}`)
    if (aTok > 0) parts.push(`asst ${humanTokens(aTok)}`)
    if (trTok > 0) parts.push(`tool ${humanTokens(trTok)}`)
    if (imgTok > 0) parts.push(`img ${humanTokens(imgTok)}`)
    if (parts.length > 0) detailLines.push(`  tok     ${parts.join(' · ')}`)
  }

  // Message breakdown — parenthetical, same style as LLM started
  let msgBreakdown = ''
  if (cms) {
    const msgParts: string[] = []
    const uCount = ((cms.user_count as number) ?? 0)
    const aCount = ((cms.assistant_count as number) ?? 0)
    const trCount = ((cms.tool_result_count as number) ?? 0)
    const imgCount = ((cms.image_count as number) ?? 0)
    if (uCount > 0) msgParts.push(`user ${uCount}`)
    if (aCount > 0) msgParts.push(`asst ${aCount}`)
    if (trCount > 0) msgParts.push(`tool ${trCount}`)
    if (imgCount > 0) msgParts.push(`img ${imgCount}`)
    if (msgParts.length > 0) msgBreakdown = ` (${msgParts.join(' · ')})`
  }

  const level = (data.level as string | undefined) ?? (data.level_name as string | undefined)
  const headerInfo = level ? [level, `${msgCount} msgs${msgBreakdown}`] : [`${msgCount} msgs${msgBreakdown}`]
  return `[COMPACT] ● compacting · ${headerInfo.join(' · ')}\n${detailLines.join('\n')}`
}

// ---------------------------------------------------------------------------
// Context compaction completed
// ---------------------------------------------------------------------------

export function formatCompactionCompleted(data: Record<string, unknown>): string {
  const result = data.result as Record<string, any> | undefined

  if (!result) return '[COMPACT] ✓ done'

  const type = (result.type as string) ?? 'done'

  switch (type) {
    case 'no_op': {
      // Show why it was skipped: include budget info if available
      const contextWindow = (data.context_window as number) ?? 0
      const estTokens = (data.estimated_tokens as number) ?? 0
      if (contextWindow > 0 && estTokens > 0) {
        const pct = ((estTokens / contextWindow) * 100).toFixed(0)
        return `[COMPACT] ✓ skipped · within budget · ~${humanTokens(estTokens)} / ${humanTokens(contextWindow)} · ${pct}%`
      }
      return '[COMPACT] ✓ skipped · within budget'
    }

    case 'run_once_cleared': {
      const saved = (result.saved_tokens as number) ?? 0
      const before = (result.before_estimated_tokens as number) ?? 0
      const after = (result.after_estimated_tokens as number) ?? 0
      const savedPct = before > 0 ? ((saved / before) * 100).toFixed(0) : '0'
      const contextWindow = (data.context_window as number) ?? 0

      const lines: string[] = []
      lines.push(`[COMPACT] ✓ cleared · −${humanTokens(saved)} · ${savedPct}%`)

      // Context window bar
      if (contextWindow > 0 && after > 0) {
        const pct = ((after / contextWindow) * 100).toFixed(0)
        const bar = renderBar(after, contextWindow, 20)
        lines.push(`  ctx     ${bar}  ~${humanTokens(after)} / ${humanTokens(contextWindow)} · ${pct}% · −${humanTokens(saved)}`)
      }

      lines.push(`  ~${humanTokens(before)} → ~${humanTokens(after)}`)
      return lines.join('\n')
    }

    case 'level_done':
    case 'level_compacted': {
      const level = (result.level as number) ?? 0
      const beforeMsgs = ((result.before_message_count as number) ?? (result.messages_before as number)) ?? 0
      const afterMsgs = ((result.after_message_count as number) ?? (result.messages_after as number)) ?? 0
      const before = ((result.before_estimated_tokens as number) ?? (result.tokens_before as number)) ?? 0
      const after = ((result.after_estimated_tokens as number) ?? (result.tokens_after as number)) ?? 0
      const saved = before - after
      const savedPct = before > 0 ? ((saved / before) * 100).toFixed(0) : '0'
      const msgsDropped = (result.messages_dropped as number) ?? 0
      const deltaMsgs = beforeMsgs - afterMsgs
      const messageDelta = deltaMsgs > 0 ? `−${deltaMsgs}` : '0'

      const allActions = result.actions as any[] | undefined
      const sorted = allActions
        ? [...allActions]
            .filter((a: any) => a.method !== 'Skipped')
            .sort((a: any, b: any) => {
              const sa = (a.before_tokens ?? 0) - (a.after_tokens ?? 0)
              const sb = (b.before_tokens ?? 0) - (b.after_tokens ?? 0)
              return sb - sa
            })
        : []

      const { bar: generatedPosBar, legend: generatedLegend } = renderPositionBar(beforeMsgs, sorted, level)
      const posBar = (result.map as string | undefined)?.trim() || generatedPosBar
      const legend = (result.legend as string | undefined) || generatedLegend

      // Summary line
      let summary: string
      if (level === 1) {
        const explicitSummary = result.result as string | undefined
        if (explicitSummary) {
          summary = explicitSummary
        } else {
          const outlineCount = sorted.filter((a: any) => a.method === 'Outline').length
          const headtailCount = sorted.filter((a: any) => a.method === 'HeadTail').length
          const parts: string[] = []
          if (outlineCount > 0) parts.push(`outlined ${outlineCount}`)
          if (headtailCount > 0) parts.push(`head-tail ${headtailCount}`)
          summary = parts.length > 0 ? `↓ ${parts.join(', ')}` : '↓ no changes'
        }
      } else if (level === 2) {
        const turnCount = sorted.length
        const totalMsgs = sorted.reduce((s: number, a: any) => s + 1 + ((a.related_count as number) ?? 0), 0)
        summary = `↓ summarized ${turnCount} turns (${totalMsgs} msgs → ${turnCount} summaries)`
      } else if (level === 3) {
        const kept = Math.max(afterMsgs - 1, 0)
        summary = `↓ dropped ${msgsDropped} msgs, kept ${kept} + 1 marker`
      } else {
        summary = '↓ no changes'
      }

      const lines: string[] = []
      lines.push(`[COMPACT] ✓ L${level} done · ${beforeMsgs} → ${afterMsgs} msgs · −${humanTokens(saved)} · ${savedPct}%`)

      // Context window bar
      const contextWindow = ((data.context_window as number) ?? (result.context_window as number)) ?? 0
      if (contextWindow > 0 && after > 0) {
        const pct = ((after / contextWindow) * 100).toFixed(0)
        const bar = renderBar(after, contextWindow, 20)
        lines.push(`  ctx     ${bar}  ~${humanTokens(after)} / ${humanTokens(contextWindow)} · ${pct}% · −${humanTokens(saved)}`)
      }

      lines.push(`  map     ${posBar}`)
      if (legend) lines.push(`  legend  ${legend}`)
      lines.push(`  result  ${summary}`)

      const explicitDetails = result.details as string[] | undefined
      if (explicitDetails && explicitDetails.length > 0) {
        const [first, ...rest] = explicitDetails
        lines.push(`  details ${first ?? ''}`)
        for (const line of rest) lines.push(`    ${line}`)
      } else if (sorted.length > 0) {
        const totalActions = allActions?.length ?? 0
        const changed = sorted.length
        let header: string
        if (level === 1) {
          header = `  details changed ${changed}/${totalActions}`
        } else if (level === 2) {
          const totalMsgs = sorted.reduce((s: number, a: any) => s + 1 + ((a.related_count as number) ?? 0), 0)
          header = `  details summarized ${changed} turns (${totalMsgs} msgs → ${changed} summaries)`
        } else if (level === 3) {
          const kept = Math.max(afterMsgs - 1, 0)
          header = `  details dropped ${msgsDropped}, kept ${kept}, marker 1`
        } else {
          header = `  details changed ${changed}`
        }
        lines.push(header)

        const TOP = 3
        const TAIL = 2
        const fmtAction = (a: any) => {
          const idx = (a.index as number) ?? 0
          const toolName = (a.tool_name as string) ?? ''
          const method = (a.method as string) ?? 'unknown'
          const bTok = (a.before_tokens as number) ?? 0
          const aTok = (a.after_tokens as number) ?? 0
          const aSaved = bTok - aTok
          if (method === 'Summarized') {
            const rc = (a.related_count as number) ?? 0
            return `    #${String(idx).padEnd(3)} turn(${1 + rc} msgs)  ~${humanTokens(bTok)} → ~${humanTokens(aTok)}  (−${humanTokens(aSaved)})`
          }
          if (method === 'Dropped') {
            const endIdx = a.end_index as number | undefined
            const idxStr = endIdx != null ? `#${idx}..#${String(endIdx).padEnd(3)}` : `#${String(idx).padEnd(3)}`
            return `    ${idxStr}  ~${humanTokens(bTok)} → ~${humanTokens(aTok)}  (−${humanTokens(aSaved)})`
          }
          return `    #${String(idx).padEnd(3)} ${toolName.padEnd(12)} ${method.padEnd(12)} ~${humanTokens(bTok)} → ~${humanTokens(aTok)}  (−${humanTokens(aSaved)})`
        }

        if (sorted.length <= TOP + TAIL) {
          for (const a of sorted) lines.push(fmtAction(a))
        } else {
          for (const a of sorted.slice(0, TOP)) lines.push(fmtAction(a))
          const omitted = sorted.length - TOP - TAIL
          lines.push(`    ... ${omitted} more ...`)
          for (const a of sorted.slice(sorted.length - TAIL)) lines.push(fmtAction(a))
        }
      }

      return lines.join('\n')
    }

    default:
      return `[COMPACT] ✓ ${type}`
  }
}
