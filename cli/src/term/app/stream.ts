import { buildError, buildRunSummary, buildToolCall, buildToolResult, buildVerboseEvent, buildAssistantLines, findSafeSplitPoint, type OutputLine } from '../../render/output.js'
import { splitMarkdownBlocks } from '../../render/markdown.js'
import { setSpinnerPhase, type SpinnerState } from '../spinner.js'
import { applyEvent } from './reducer.js'
import type { AppState } from './state.js'
import type { RunEvent } from '../../native/index.js'

const PACE_INTERVAL_MS = 30

export interface StreamMachineState {
  appState: AppState
  spinnerState: SpinnerState
  pendingText: string
  toolProgress: string
  streamingText: string
  prefixEmitted: boolean
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

export function createStreamMachineState(appState: AppState, spinnerState: SpinnerState): StreamMachineState {
  return {
    appState,
    spinnerState,
    pendingText: '',
    toolProgress: '',
    streamingText: '',
    prefixEmitted: false,
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
      const { completed, pending } = splitMarkdownBlocks(state.streamingText)
      if (completed) {
        const builtLines = buildAssistantLines(completed)
        commitLines.push(...builtLines)
        writeLines.push(...builtLines)
        state = { ...state, streamingText: pending }
      }

      // If streaming text is very long (exceeds visible area), force-split
      if (state.streamingText.split('\n').length > ctx.termRows - 8) {
        const splitAt = findSafeSplitPoint(state.streamingText)
        if (splitAt > 0 && splitAt < state.streamingText.length) {
          const chunk = state.streamingText.slice(0, splitAt)
          const rest = state.streamingText.slice(splitAt)
          const builtLines = buildAssistantLines(chunk)
          commitLines.push(...builtLines)
          writeLines.push(...builtLines)
          state = { ...state, streamingText: rest }
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
    state = {
      ...state,
      toolProgress: '',
      spinnerState: setSpinnerPhase(state.spinnerState, 'executing', toolName),
    }
    suppressToolStarted = toolName === 'ask_user'
    rerenderStatus = true
  }

  if (event.kind === 'tool_progress') {
    const text = p.text as string | undefined
    if (text) {
      state = { ...state, toolProgress: text }
      rerenderStatus = true
    }
  }

  if (event.kind === 'tool_finished') {
    const toolName = (p.tool_name as string) ?? 'unknown'
    state = {
      ...state,
      toolProgress: '',
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

export function flushStreaming(state: StreamMachineState): { state: StreamMachineState; lines: OutputLine[] } {
  if (!state.streamingText.trim()) {
    return {
      state: { ...state, streamingText: '', pendingText: '' },
      lines: [],
    }
  }

  const lines = buildAssistantLines(state.streamingText)
  return {
    state: { ...state, streamingText: '', pendingText: '' },
    lines,
  }
}
