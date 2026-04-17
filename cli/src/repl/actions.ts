import React from 'react'
import { Agent, QueryStream } from '../native/index.js'
import type { ContentBlock } from '../native/index.js'
import type { PastedImage } from '../components/PromptInput.js'
import type { AppState } from '../state/app.js'
import { applyEvent } from '../state/reducer.js'
import type { OutputLine } from '../render/output.js'
import type { AskUserRequest } from '../state/types.js'
import {
  buildUserMessage,
  buildAssistantLines,
  buildToolCall,
  buildToolResult,
  buildVerboseEvent,
  buildError,
  buildRunSummary,
  findSafeSplitPoint,
} from '../render/output.js'
import { splitMarkdownBlocks } from '../render/markdown.js'
import { ScreenLog } from '../session/screen-log.js'

export async function runLogTurn(
  forked: import('../native/index.js').ForkedAgent,
  prompt: string,
  setOutputLines: React.Dispatch<React.SetStateAction<OutputLine[]>>,
  setPendingText: React.Dispatch<React.SetStateAction<string>>,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
) {
  setState(prev => ({ ...prev, isLoading: true }))
  const appendLines = (lines: OutputLine[]) => {
    if (lines.length > 0) setOutputLines(prev => [...prev, ...lines])
  }
  appendLines(buildUserMessage(prompt))

  try {
    const stream = await forked.query(prompt)
    let streamingText = ''

    const commitStreamingText = () => {
      if (streamingText.trim()) {
        appendLines(buildAssistantLines(streamingText))
      }
      streamingText = ''
    }

    for await (const event of stream) {
      switch (event.kind) {
        case 'assistant_delta': {
          const delta = event.payload?.delta as string | undefined
          if (delta) {
            streamingText += delta
            const lastNl = streamingText.lastIndexOf('\n')
            if (lastNl >= 0) {
              const complete = streamingText.slice(0, lastNl + 1)
              streamingText = streamingText.slice(lastNl + 1)
              if (complete.trim()) {
                appendLines(buildAssistantLines(complete))
              }
            }
          }
          break
        }
        case 'tool_started': {
          commitStreamingText()
          const toolName = (event.payload?.tool_name as string) ?? 'tool'
          appendLines(buildToolCall(toolName, event.payload))
          break
        }
        case 'tool_completed': {
          const p = event.payload ?? {}
          const toolName = (p.tool_name as string) ?? 'tool'
          const args = (p.args as Record<string, unknown>) ?? {}
          const details = p.details as Record<string, any> | undefined
          const mergedArgs = details?.diff ? { ...args, diff: details.diff } : args
          const status = p.error != null ? 'error' as const : 'done' as const
          const content = p.content as string | undefined
          const durationMs = p.duration_ms as number | undefined
          appendLines(buildToolResult(toolName, mergedArgs, status, content, durationMs))
          break
        }
        default:
          break
      }
    }
    commitStreamingText()
  } catch (err: any) {
    appendLines(buildError(`Log query failed: ${err?.message ?? err}`))
  }
  setState(prev => ({ ...prev, isLoading: false }))
}

export async function runQuery(
  agent: Agent,
  text: string,
  images: PastedImage[] | undefined,
  sessionId: string | null,
  streamRef: React.MutableRefObject<QueryStream | null>,
  streamGenRef: React.MutableRefObject<number>,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  setOutputLines: React.Dispatch<React.SetStateAction<OutputLine[]>>,
  setPendingText: React.Dispatch<React.SetStateAction<string>>,
  setToolProgress: React.Dispatch<React.SetStateAction<string>>,
  stateRef: React.MutableRefObject<AppState>,
  toolMode?: string,
) {
  const gen = ++streamGenRef.current
  let streamingText = ''
  let prefixEmitted = false
  let localState = stateRef.current

  // Throttle setState for assistant_delta to ~60ms to avoid re-render storms
  let deltaFlushTimer: ReturnType<typeof setTimeout> | null = null
  let deltaStateDirty = false
  const cancelDeltaFlush = () => {
    if (deltaFlushTimer) { clearTimeout(deltaFlushTimer); deltaFlushTimer = null }
    deltaStateDirty = false
  }
  const flushDeltaState = () => {
    if (deltaFlushTimer) { clearTimeout(deltaFlushTimer); deltaFlushTimer = null }
    if (deltaStateDirty && gen === streamGenRef.current) {
      setState(() => localState)
      deltaStateDirty = false
    }
  }

  let screenLog: ScreenLog | null = null
  const appendLines = (lines: OutputLine[]) => {
    if (lines.length === 0) return
    setOutputLines((prev) => [...prev, ...lines])
    try { screenLog?.writeLines(lines) } catch { /* ignore */ }
  }

  const commitStreamingText = () => {
    if (!streamingText.trim()) {
      streamingText = ''
      setPendingText('')
      return
    }
    appendLines(buildAssistantLines(streamingText))
    streamingText = ''
    setPendingText('')
  }

  try {
    let stream: QueryStream
    if (images && images.length > 0) {
      const content: ContentBlock[] = [
        ...(text ? [{ type: 'text' as const, text }] : []),
        ...images.map((img) => ({
          type: 'image' as const,
          data: img.base64,
          mimeType: img.mediaType,
        })),
      ]
      const contentJson = JSON.stringify(content)
      stream = await agent.query('', sessionId ?? undefined, toolMode, contentJson)
    } else {
      stream = await agent.query(text, sessionId ?? undefined, toolMode)
    }
    if (gen !== streamGenRef.current) { stream.abort(); return }
    streamRef.current = stream

    try {
      screenLog = new ScreenLog(stream.sessionId)
    } catch { /* ignore log failures */ }

    appendLines(buildUserMessage(text, images?.length))

    for await (const event of stream) {
      if (gen !== streamGenRef.current) break

      const p = event.payload as Record<string, any>
      // ask_user is a synthetic event from NAPI — skip reducer
      let nextState = event.kind === 'ask_user' ? localState : applyEvent(localState, event)

      if (localState.verbose
        && (event.kind === 'llm_call_started' || event.kind === 'context_compaction_started')) {
        commitStreamingText()
        const newEvents = nextState.verboseEvents.slice(localState.verboseEvents.length)
        for (const evt of newEvents) appendLines(buildVerboseEvent(evt.text))
      }

      if (event.kind === 'assistant_delta') {
        const delta = p.delta as string | undefined
        if (delta) {
          streamingText += delta
          if (!prefixEmitted) {
            const trimmed = streamingText.replace(/^[\n\r]+/, '')
            if (trimmed.length > 0) {
              streamingText = trimmed
              prefixEmitted = true
            }
          }
          const { completed, pending } = splitMarkdownBlocks(streamingText)
          if (completed) {
            appendLines(buildAssistantLines(completed))
            streamingText = pending
          }
          const termRows = process.stdout.rows ?? 24
          if (streamingText.split('\n').length > termRows - 8) {
            const splitAt = findSafeSplitPoint(streamingText)
            if (splitAt > 0 && splitAt < streamingText.length) {
              const chunk = streamingText.slice(0, splitAt)
              streamingText = streamingText.slice(splitAt)
              appendLines(buildAssistantLines(chunk))
            }
          }
          setPendingText(streamingText)
        }
      }

      if (event.kind === 'assistant_completed' || event.kind === 'turn_started') {
        commitStreamingText()
      }

      if (localState.verbose
        && (event.kind === 'llm_call_completed' || event.kind === 'context_compaction_completed')) {
        commitStreamingText()
        const newEvents = nextState.verboseEvents.slice(localState.verboseEvents.length)
        for (const evt of newEvents) appendLines(buildVerboseEvent(evt.text))
      }

      if (event.kind === 'tool_started') {
        commitStreamingText()
        setToolProgress('')
        const toolName = p.tool_name ?? 'unknown'
        const args = p.args ?? {}
        const previewCommand = p.preview_command as string | undefined
        // Don't show tool_started line for ask_user — the UI component handles it
        if (toolName !== 'ask_user') {
          appendLines(buildToolCall(toolName, args, previewCommand))
        }
      }

      // ask_user: synthetic event from NAPI bridge — set state so REPL renders the UI
      if (event.kind === 'ask_user') {
        commitStreamingText()
        const questions = (event.payload as any)?.questions
        if (questions) {
          const req = { questions } as AskUserRequest
          localState = { ...localState, askUserRequest: req }
          setState(() => localState)
          // Skip the localState = nextState assignment below
          continue
        }
      }

      if (event.kind === 'tool_progress') {
        const text = p.text as string | undefined
        if (text) setToolProgress(text)
      }

      if (event.kind === 'tool_finished') {
        setToolProgress('')
        const toolName = p.tool_name ?? 'unknown'
        if (toolName !== 'ask_user') {
          const args = p.args ?? {}
          const details = p.details as Record<string, any> | undefined
          const mergedArgs = details?.diff ? { ...args, diff: details.diff } : args
          const status = p.is_error ? 'error' as const : 'done' as const
          appendLines(buildToolResult(toolName, mergedArgs, status, p.content, p.duration_ms))
        }
        // Clear ask_user state when ask_user tool finishes
        if (toolName === 'ask_user') {
          nextState = { ...nextState, askUserRequest: null }
        }
      }

      if (event.kind === 'error') {
        commitStreamingText()
        appendLines(buildError(p.message as string ?? 'Unknown error'))
      }

      if (event.kind === 'run_finished' && localState.verbose) {
        commitStreamingText()
        appendLines(buildRunSummary(nextState.currentRunStats))
      }

      localState = nextState
      if (event.kind === 'assistant_delta') {
        deltaStateDirty = true
        if (!deltaFlushTimer) {
          deltaFlushTimer = setTimeout(flushDeltaState, 60)
        }
      } else {
        flushDeltaState()
        setState(() => localState)
      }
    }

    if (gen === streamGenRef.current) {
      commitStreamingText()
      flushDeltaState()
    }
  } catch (err: any) {
    if (gen === streamGenRef.current) {
      commitStreamingText()
      flushDeltaState()
    }
    if (gen !== streamGenRef.current) return
    const errLines = buildError(err?.message ?? String(err))
    appendLines(errLines)
    setState((prev) => ({
      ...prev,
      isLoading: false,
      error: err?.message ?? String(err),
    }))
  } finally {
    cancelDeltaFlush()
    streamRef.current = null
    if (gen === streamGenRef.current) {
      setState((prev) => prev.isLoading ? { ...prev, isLoading: false } : prev)
    }
  }
}
