import { buildToolCard } from '../src/render/output.js'
import { describe, expect, test } from 'bun:test'
import { decodeQueryEvent, isHostToolEvent } from '../src/native/contracts/query-event.js'
import { applyEvent } from '../src/term/app/reducer.js'
import { createInitialState } from '../src/term/app/state.js'
import { assistantToolCalls } from '../src/term/app/assistant-content.js'
import { createStreamMachineState, reduceRunEvent } from '../src/term/app/stream.js'
import { createSpinnerState } from '../src/term/spinner.js'
import currentPayloads from './fixtures/contracts/run-payloads-current.json'
import type { AppState } from '../src/term/app/state.js'

function apply(state: AppState, kind: string, payload: Record<string, unknown>): AppState {
  const event = decodeQueryEvent(JSON.stringify({
    event_id: 'e', run_id: 'r', session_id: 's', turn: 1, created_at: '', kind, payload,
  }))
  if (isHostToolEvent(event)) throw new Error('expected run event')
  return applyEvent(state, event)
}

function queued() {
  return apply(createInitialState('model', '/tmp'), 'assistant_tool_call', {
    content_index: 0, tool_call_id: 'call', tool_name: 'custom', phase: 'end', args: { path: 'file' },
  })
}

describe('typed run reducer', () => {
  test('bash countdown updates in place and disappears when the tool finishes', () => {
    const queuedState = apply(createInitialState('model', '/tmp'), 'assistant_tool_call', {
      content_index: 0, tool_call_id: 'bash', tool_name: 'bash', phase: 'end', args: { command: 'sleep 30' },
    })
    let state = apply(queuedState, 'tool_started', {
      tool_call_id: 'bash', tool_name: 'bash', args: { command: 'sleep 30' },
    })
    for (const remaining of [10, 9, 1]) {
      const text = `Moves to background in ${remaining}s; command keeps running.`
      state = apply(state, 'tool_progress', { tool_call_id: 'bash', tool_name: 'bash', text })
      const tool = assistantToolCalls(state.currentAssistantContent)[0]!
      expect(tool.progress).toBe(text)
      const lines = buildToolCard(tool).map(line => line.text).join('\n')
      expect(lines).toContain(text)
      expect(lines.match(/Moves to background/g)).toHaveLength(1)
    }
    state = apply(state, 'tool_finished', {
      tool_call_id: 'bash', tool_name: 'bash', content: 'Background task started', is_error: false,
      details: { task_id: 'task', status: 'running' }, duration_ms: 10000,
    })
    const tool = assistantToolCalls(state.currentAssistantContent)[0]!
    expect(buildToolCard(tool).map(line => line.text).join('\n')).not.toContain('Moves to background')
  })

  test('current Rust payload fixtures flow through decoder, stream and reducer', () => {
    let state = createStreamMachineState(createInitialState('model', '/tmp'), createSpinnerState())
    for (const fixture of currentPayloads) {
      const event = decodeQueryEvent(JSON.stringify({
        event_id: 'e', run_id: 'r', session_id: 's', turn: 1, created_at: '', ...fixture,
      }))
      if (isHostToolEvent(event)) throw new Error('expected run event')
      state = reduceRunEvent(state, event, { termRows: 24 }).state
      for (const tool of assistantToolCalls(state.appState.currentAssistantContent)) {
        expect(typeof tool.args).toBe('object')
        expect(tool.args).not.toBeNull()
        expect(Array.isArray(tool.args)).toBe(false)
      }
    }
    expect(state.appState.isLoading).toBe(false)
    expect(state.appState.error).toBe('fixture failure')
  })

  test('non-object final tool arguments cannot enter UI object state', () => {
    for (const args of [null, false, 1, 'text', []]) {
      const state = apply(queued(), 'assistant_tool_call', {
        content_index: 0, tool_call_id: 'call', tool_name: 'custom', phase: 'end', args,
      })
      expect(assistantToolCalls(state.currentAssistantContent)[0]?.args).toEqual({ path: 'file' })
    }
  })

  test('execution preserves existing arguments when wire JSON is not an object', () => {
    for (const args of [null, false, 1, 'text', []]) {
      const state = apply(queued(), 'tool_started', { tool_call_id: 'call', tool_name: 'custom', args })
      const tool = assistantToolCalls(state.currentAssistantContent)[0]
      expect(tool?.args).toEqual({ path: 'file' })
      expect(tool?.status).toBe('running')
    }
    const state = apply(queued(), 'tool_started', {
      tool_call_id: 'call', tool_name: 'custom', args: { path: 'changed' },
    })
    expect(assistantToolCalls(state.currentAssistantContent)[0]?.args).toEqual({ path: 'changed' })
  })

  test('authoritative completion and streaming use the same argument projection', () => {
    for (const input of [null, false, 'text', []]) {
      const state = apply(createInitialState('model', '/tmp'), 'assistant_completed', {
        content: [{ type: 'tool_call', id: 'call', name: 'custom', input }], stop_reason: 'tool_use',
      })
      expect(assistantToolCalls(state.currentAssistantContent)[0]?.args).toEqual({})
    }
  })

  test('unknown events are inert and optional usage defaults remain zero', () => {
    const initial = createInitialState('model', '/tmp')
    expect(apply(initial, 'future_event', { data: true })).toBe(initial)
    const state = apply(initial, 'llm_call_completed', { turn: 1, attempt: 0, usage: { input: 3, output: 4 } })
    expect(state.sessionTokens.inputTokens).toBe(3)
    expect(state.sessionTokens.outputTokens).toBe(4)
    expect(state.sessionTokens.cacheReadTokens).toBe(0)
    expect(state.sessionTokens.cacheWriteTokens).toBe(0)
  })

  test('legacy retry and compaction projections remain supported', () => {
    const initial = createInitialState('model', '/tmp')
    // Older internal/replay callers need not use the current wire decoder.
    const event = { event_id: 'e', run_id: 'r', session_id: 's', turn: 1, created_at: '' }
    const retried = applyEvent(initial, { ...event, kind: 'api_retry', payload: { attempt: 1, delay_ms: 100, error: 'busy' } })
    expect(retried.verboseEvents.at(-1)?.kind).toBe('llm_retry')
    const compacted = applyEvent(initial, { ...event, kind: 'context_compaction_completed', payload: {
      result: { type: 'level_compacted', level: 2, before_estimated_tokens: 100, after_estimated_tokens: 25 },
    } })
    expect(compacted.currentRunStats.compactHistory).toEqual([{ level: 2, beforeTokens: 100, afterTokens: 25 }])
  })
})
