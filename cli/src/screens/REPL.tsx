/**
 * REPL screen — main interactive conversation view.
 * Orchestrates Agent queries, event streaming, and UI rendering.
 */

import React, { useState, useCallback, useRef, useEffect } from 'react'
import { Box, Text, useApp, useInput } from 'ink'
import { Agent, type RunEvent, QueryStream } from '../native/index.js'
import { type AppState, createInitialState, applyEvent, type UIMessage } from '../state/AppState.js'
import { Message } from '../components/Message.js'
import { Spinner } from '../components/Spinner.js'
import { PromptInput } from '../components/PromptInput.js'
import { StreamingText } from '../components/StreamingText.js'
import { StatusLine } from '../components/StatusLine.js'
import { ToolCallDisplay } from '../components/ToolCallDisplay.js'

interface REPLProps {
  agent: Agent
}

export function REPL({ agent }: REPLProps) {
  const { exit } = useApp()
  const [state, setState] = useState<AppState>(() =>
    createInitialState(agent.model, agent.cwd)
  )
  const streamRef = useRef<QueryStream | null>(null)
  const sessionIdRef = useRef<string | null>(null)

  // Keep sessionId ref in sync for use in async callbacks
  useEffect(() => {
    sessionIdRef.current = state.sessionId
  }, [state.sessionId])

  // Global Ctrl+C handler during loading
  useInput((_ch, key) => {
    if (key.ctrl && _ch === 'c') {
      if (streamRef.current) {
        streamRef.current.abort()
        streamRef.current = null
        setState((prev) => ({
          ...prev,
          isLoading: false,
          currentStreamText: '',
          currentThinkingText: '',
          activeToolCalls: new Map(),
        }))
      } else if (!state.isLoading) {
        exit()
      }
    }
  }, { isActive: state.isLoading })

  const handleSubmit = useCallback(
    (text: string) => {
      // Add user message immediately
      const userMsg: UIMessage = {
        id: `user-${Date.now()}`,
        role: 'user',
        text,
        timestamp: Date.now(),
      }
      setState((prev) => ({
        ...prev,
        messages: [...prev.messages, userMsg],
        isLoading: true,
        error: null,
        currentStreamText: '',
        currentThinkingText: '',
        activeToolCalls: new Map(),
      }))

      // Fire-and-forget async query — state updates via setState
      runQuery(agent, text, sessionIdRef.current, streamRef, setState)
    },
    [agent]
  )

  const handleInterrupt = useCallback(() => {
    if (streamRef.current) {
      streamRef.current.abort()
      streamRef.current = null
      setState((prev) => ({
        ...prev,
        isLoading: false,
        currentStreamText: '',
        currentThinkingText: '',
        activeToolCalls: new Map(),
      }))
    } else {
      exit()
    }
  }, [exit])

  const hasStreamText = state.currentStreamText.length > 0
  const hasThinkingText = state.currentThinkingText.length > 0
  const hasActiveTools = state.activeToolCalls.size > 0

  return (
    <Box flexDirection="column" padding={0}>
      {/* Banner */}
      <Banner model={state.model} cwd={state.cwd} />

      {/* Message history */}
      {state.messages.map((msg) => (
        <Message key={msg.id} message={msg} />
      ))}

      {/* Streaming response */}
      {state.isLoading && (hasStreamText || hasThinkingText) && (
        <StreamingText
          text={state.currentStreamText}
          thinkingText={state.currentThinkingText}
        />
      )}

      {/* Active tool calls */}
      {state.isLoading && hasActiveTools && (
        <ToolCallDisplay tools={state.activeToolCalls} />
      )}

      {/* Spinner — only when waiting with no output yet */}
      {state.isLoading && !hasStreamText && !hasThinkingText && !hasActiveTools && (
        <Spinner text="Thinking..." />
      )}

      {/* Error */}
      {state.error && (
        <Box marginBottom={1}>
          <Text color="red">Error: {state.error}</Text>
        </Box>
      )}

      {/* Prompt input */}
      <PromptInput
        model={state.model}
        isLoading={state.isLoading}
        onSubmit={handleSubmit}
        onInterrupt={handleInterrupt}
      />

      {/* Status line */}
      <StatusLine
        sessionId={state.sessionId}
        model={state.model}
        cwd={state.cwd}
        messageCount={state.messages.length}
      />
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Async query runner (outside component to avoid closure issues)
// ---------------------------------------------------------------------------

async function runQuery(
  agent: Agent,
  text: string,
  sessionId: string | null,
  streamRef: React.MutableRefObject<QueryStream | null>,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
) {
  try {
    const stream = await agent.query(text, sessionId ?? undefined)
    streamRef.current = stream

    for await (const event of stream) {
      setState((prev) => applyEvent(prev, event))
    }
  } catch (err: any) {
    setState((prev) => ({
      ...prev,
      isLoading: false,
      error: err?.message ?? String(err),
    }))
  } finally {
    streamRef.current = null
  }
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

function Banner({ model, cwd }: { model: string; cwd: string }) {
  const shortCwd = cwd.replace(process.env.HOME ?? '', '~')

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box>
        <Text backgroundColor="#5a2d82" color="white" bold>
          {' ◆ bendclaw '}
        </Text>
        <Text dimColor> v0.1.0</Text>
      </Box>
      <Box>
        <Text dimColor>
          {shortCwd} · {model}
        </Text>
      </Box>
      <Box>
        <Text dimColor>
          Type a message to start. Ctrl+C to interrupt or exit.
        </Text>
      </Box>
    </Box>
  )
}
