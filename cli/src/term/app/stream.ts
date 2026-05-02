import { buildError, buildRunSummary, buildToolCall, buildToolProgress, buildToolResult, buildVerboseEvent, buildAssistantLines, type OutputLine } from '../../render/output.js'
import { findStreamingCommitPoint } from '../../render/markdown.js'
import { setSpinnerPhase, type SpinnerState } from '../spinner.js'
import { applyEvent } from './reducer.js'
import type { AppState } from './state.js'
import type { RunEvent } from '../../native/index.js'

const PACE_INTERVAL_MS = 30
let sepId = 0

export interface StreamMachineState {
  appState: AppState
  spinnerState: SpinnerState
  pendingText: string
  toolProgress: string
  lastToolProgress: string
  streamingText: string
  prefixEmitted: boolean
  assistantCommitted: boolean
  lastPendingRender: number
}

export interface StreamContext {
  termRows: number
}

export interface StreamUpdate {
  state: StreamMachineState
  commitLines: OutputLine[]
  writeLines: OutputLine[]
  rerenderStatus: boolean
  suppressToolStarted: boolean
  suppressToolFinished: boolean
}

function isHeartbeatProgress(text: string): boolean {
  return /^Running\.\.\. \d+s$/.test(text.trim())
}

export function createStreamMachineState(appState: AppState, spinnerState: SpinnerState): StreamMachineState {
  return {
    appState,
    spinnerState,
    pendingText: '',
    toolProgress: '',
    lastToolProgress: '',
    streamingText: '',
    prefixEmitted: false,
    assistantCommitted: false,
    lastPendingRender: 0,
  }
}

export function reduceRunEvent(prev: StreamMachineState, event: RunEvent, ctx: StreamContext): StreamUpdate {
  const p = (event.payload ?? {}) as Record<string, any>
  let state = event.kind === 'ask_user' ? prev : { ...prev, appState: applyEvent(prev.appState, event) }
  const commitLines: OutputLine[] = []
  const writeLines: OutputLine[] = []
  let rerenderStatus = false
  let suppressToolStarted = false
  let suppressToolFinished = false

  if (prev.appState.verbose && (event.kind === 'llm_call_started' || event.kind === 'context_compaction_started')) {
    const flushed = flushStreaming(state)
    state = { ...flushed.state, toolProgress: '', lastToolProgress: '' }
    commitLines.push(...flushed.lines)
    writeLines.push(...flushed.lines)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      const verboseLines = buildVerboseEvent(evt.text)
      commitLines.push(...verboseLines)
      writeLines.push(...verboseLines)
    }
  }

  if (event.kind === 'assistant_delta') {
    const delta = p.delta as string | undefined
    if (delta) {
      state = { ...state, streamingText: state.streamingText + delta }
      if (!state.prefixEmitted) {
        const trimmed = state.streamingText.replace(/^[\n\r]+/, '')
        if (trimmed.length > 0) {
          state = { ...state, streamingText: trimmed, prefixEmitted: true }
        }
      }

      state = {
        ...state,
        spinnerState: {
          ...state.spinnerState,
          lastTokenAt: Date.now(),
          streaming: true,
          tokenCount: state.spinnerState.tokenCount + 1,
        },
      }

      // Commit completed markdown blocks directly to scroll area.
      const commitPoint = findStreamingCommitPoint(state.streamingText)
      if (commitPoint > 0) {
        const completed = state.streamingText.slice(0, commitPoint)
        const pending = state.streamingText.slice(commitPoint)
        const builtLines = buildAssistantLines(completed)
        // Insert blank line between consecutive committed chunks so block
        // spacing matches the full-document render (trim strips it otherwise).
        if (state.assistantCommitted && builtLines.length > 0) {
          const sep: OutputLine = { id: `sep-${sepId++}`, kind: 'assistant', text: '' }
          commitLines.push(sep)
          writeLines.push(sep)
        }
        commitLines.push(...builtLines)
        writeLines.push(...builtLines)
        state = { ...state, streamingText: pending, assistantCommitted: true }
      }

      // Force-split when pending text exceeds a fraction of the visible area
      // so content flows into the scroll zone (append) instead of staying in
      // the status area (re-render in place). Only split at markdown-safe
      // boundaries; otherwise keep the whole growing block dynamic.
      const pendingLineCount = state.streamingText.split('\n').length
      const forceThreshold = Math.max(4, Math.floor(ctx.termRows / 3))
      if (pendingLineCount > forceThreshold) {
        const splitAt = findStreamingCommitPoint(state.streamingText)
        if (splitAt > 0 && splitAt < state.streamingText.length) {
          const chunk = state.streamingText.slice(0, splitAt)
          const rest = state.streamingText.slice(splitAt)
          const builtLines = buildAssistantLines(chunk)
          if (state.assistantCommitted && builtLines.length > 0) {
            const sep: OutputLine = { id: `sep-${sepId++}`, kind: 'assistant', text: '' }
            commitLines.push(sep)
            writeLines.push(sep)
          }
          commitLines.push(...builtLines)
          writeLines.push(...builtLines)
          state = { ...state, streamingText: rest, assistantCommitted: true }
        }
      }

      // Update pendingText for status area (shows last line of in-progress text)
      state = { ...state, pendingText: state.streamingText }
      rerenderStatus = true
    }
  }

  if (event.kind === 'assistant_completed' || event.kind === 'turn_started') {
    const flushed = flushStreaming(state)
    state = {
      ...flushed.state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: { ...flushed.state.spinnerState, streaming: false },
    }
    commitLines.push(...flushed.lines)
    writeLines.push(...flushed.lines)
    rerenderStatus = true
  }

  if (prev.appState.verbose && (event.kind === 'llm_call_completed' || event.kind === 'context_compaction_completed')) {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    writeLines.push(...flushed.lines)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      const verboseLines = buildVerboseEvent(evt.text)
      commitLines.push(...verboseLines)
      writeLines.push(...verboseLines)
    }
  }

  if (event.kind === 'tool_started') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    writeLines.push(...flushed.lines)
    const toolName = (p.tool_name as string) ?? 'unknown'
    // ask_user is waiting for user input, not "executing" — keep thinking phase
    const spinnerPhase = toolName === 'ask_user' ? 'thinking' : 'executing'
    state = {
      ...state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: setSpinnerPhase(state.spinnerState, spinnerPhase, toolName),
    }
    suppressToolStarted = toolName === 'ask_user'
    rerenderStatus = true
  }

  if (event.kind === 'tool_progress') {
    const text = p.text as string | undefined
    if (text) {
      state = isHeartbeatProgress(text)
        ? { ...state, toolProgress: '' }
        : { ...state, toolProgress: text, lastToolProgress: text }
      rerenderStatus = true
    }
  }

  if (event.kind === 'tool_finished') {
    const toolName = (p.tool_name as string) ?? 'unknown'
    state = {
      ...state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: setSpinnerPhase(state.spinnerState, 'thinking'),
    }
    suppressToolFinished = toolName === 'ask_user'
    rerenderStatus = true
  }

  if (event.kind === 'error') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    writeLines.push(...flushed.lines)
    commitLines.push(...buildError((p.message as string) ?? 'Unknown error'))
  }

  if (event.kind === 'run_finished' && prev.appState.verbose) {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    writeLines.push(...flushed.lines)
    commitLines.push(...buildRunSummary(state.appState.currentRunStats))
  }

  return {
    state,
    commitLines,
    writeLines,
    rerenderStatus,
    suppressToolStarted,
    suppressToolFinished,
  }
}

export function buildToolFinishedLines(event: RunEvent, expanded?: boolean): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const args = (p.args as Record<string, unknown>) ?? {}
  const details = p.details as Record<string, any> | undefined
  const mergedArgs = details?.diff ? { ...args, diff: details.diff } : args
  const status = p.is_error ? 'error' as const : 'done' as const
  return buildToolResult(toolName, mergedArgs, status, p.content as string | undefined, p.duration_ms as number | undefined, expanded)
}

export function buildToolStartedLines(event: RunEvent): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const previewCommand = p.preview_command as string | undefined
  return buildToolCall(toolName, (p.args as Record<string, unknown>) ?? {}, previewCommand)
}

export function buildToolProgressLines(event: RunEvent, expanded?: boolean): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const text = (p.text as string) ?? ''
  return text ? buildToolProgress(toolName, text, expanded) : []
}

export function flushStreaming(state: StreamMachineState): { state: StreamMachineState; lines: OutputLine[] } {
  if (!state.streamingText.trim()) {
    return {
      state: { ...state, streamingText: '', pendingText: '', assistantCommitted: false },
      lines: [],
    }
  }

  const lines = buildAssistantLines(state.streamingText)
  if (state.assistantCommitted && lines.length > 0) {
    lines.unshift({ id: `sep-${sepId++}`, kind: 'assistant', text: '' })
  }
  return {
    state: { ...state, streamingText: '', pendingText: '', assistantCommitted: false },
    lines,
  }
}
