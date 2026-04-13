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
import { ToolCallDisplay } from '../components/ToolCallDisplay.js'
import { RunSummary } from '../components/RunSummary.js'
import { isSlashCommand, resolveCommand, formatHelp } from '../commands/index.js'

interface REPLProps {
  agent: Agent
}

export function REPL({ agent }: REPLProps) {
  const { exit } = useApp()
  const [state, setState] = useState<AppState>(() =>
    createInitialState(agent.model, agent.cwd)
  )
  const [systemMessages, setSystemMessages] = useState<SystemMsg[]>([])
  const streamRef = useRef<QueryStream | null>(null)
  const sessionIdRef = useRef<string | null>(null)

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
        pushSystem(setSystemMessages, 'info', 'Interrupted.')
      }
    }
  }, { isActive: state.isLoading })

  const handleSubmit = useCallback(
    (text: string) => {
      setSystemMessages([])

      if (isSlashCommand(text)) {
        handleSlashCommand(text, agent, state, setState, setSystemMessages, exit)
        return
      }

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

      runQuery(agent, text, sessionIdRef.current, streamRef, setState)
    },
    [agent, state, exit]
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
      pushSystem(setSystemMessages, 'info', 'Interrupted.')
    } else {
      exit()
    }
  }, [exit])

  const handleToggleVerbose = useCallback(() => {
    setState((prev) => ({ ...prev, verbose: !prev.verbose }))
  }, [])

  const hasStreamText = state.currentStreamText.length > 0
  const hasThinkingText = state.currentThinkingText.length > 0
  const hasActiveTools = state.activeToolCalls.size > 0

  // Find the last run stats for display
  const lastRunStats = state.verbose
    ? [...state.messages].reverse().find((m) => m.runStats)?.runStats
    : undefined

  return (
    <Box flexDirection="column" padding={0}>
      <Banner model={state.model} cwd={state.cwd} />

      {/* Message history */}
      {state.messages.map((msg) => (
        <React.Fragment key={msg.id}>
          <Message message={msg} verbose={state.verbose} />
          {/* Show run summary after the last assistant message of each run */}
          {state.verbose && msg.runStats && (
            <RunSummary stats={msg.runStats} />
          )}
        </React.Fragment>
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

      {/* Spinner */}
      {state.isLoading && !hasStreamText && !hasThinkingText && !hasActiveTools && (
        <Spinner text="Thinking..." />
      )}

      {/* Error */}
      {state.error && (
        <Box marginBottom={1}>
          <Text color="red">Error: {state.error}</Text>
        </Box>
      )}

      {/* System messages */}
      {systemMessages.map((msg, i) => (
        <Box key={i}>
          <Text
            color={msg.level === 'error' ? 'red' : msg.level === 'warn' ? 'yellow' : undefined}
            dimColor={msg.level === 'info'}
          >
            {msg.text}
          </Text>
        </Box>
      ))}
      {systemMessages.length > 0 && <Text>{''}</Text>}

      {/* Prompt input with bordered box + footer */}
      <PromptInput
        model={state.model}
        isLoading={state.isLoading}
        verbose={state.verbose}
        onSubmit={handleSubmit}
        onInterrupt={handleInterrupt}
        onToggleVerbose={handleToggleVerbose}
      />
    </Box>
  )
}

// ---------------------------------------------------------------------------
// System messages
// ---------------------------------------------------------------------------

interface SystemMsg {
  level: 'info' | 'warn' | 'error'
  text: string
}

function pushSystem(
  setter: React.Dispatch<React.SetStateAction<SystemMsg[]>>,
  level: SystemMsg['level'],
  text: string,
) {
  setter((prev) => [...prev, { level, text }])
}

// ---------------------------------------------------------------------------
// Slash command handler
// ---------------------------------------------------------------------------

async function handleSlashCommand(
  input: string,
  agent: Agent,
  state: AppState,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  setSystem: React.Dispatch<React.SetStateAction<SystemMsg[]>>,
  exit: () => void,
) {
  const resolved = resolveCommand(input)

  if (resolved.kind === 'unknown') {
    pushSystem(setSystem, 'error', `Unknown command: ${input}`)
    pushSystem(setSystem, 'info', 'Type /help for available commands')
    return
  }

  if (resolved.kind === 'ambiguous') {
    pushSystem(setSystem, 'warn', `Ambiguous command: ${resolved.candidates.join(', ')}`)
    pushSystem(setSystem, 'info', 'Type more characters or /help for commands')
    return
  }

  const { name, args } = resolved

  switch (name) {
    case '/help':
      pushSystem(setSystem, 'info', formatHelp())
      break

    case '/exit':
      exit()
      break

    case '/clear':
      setState((prev) => ({ ...prev, messages: [] }))
      pushSystem(setSystem, 'info', 'Messages cleared.')
      break

    case '/new':
      setState((prev) => ({
        ...prev,
        messages: [],
        sessionId: null,
        error: null,
      }))
      pushSystem(setSystem, 'info', 'New session started.')
      break

    case '/model': {
      if (args.trim()) {
        agent.model = args.trim()
        setState((prev) => ({ ...prev, model: args.trim() }))
        pushSystem(setSystem, 'info', `Model → ${args.trim()}`)
      } else {
        pushSystem(setSystem, 'info', `Current model: ${state.model}`)
      }
      break
    }

    case '/verbose': {
      setState((prev) => ({ ...prev, verbose: !prev.verbose }))
      const newVerbose = !state.verbose
      pushSystem(setSystem, 'info', `Verbose mode ${newVerbose ? 'on' : 'off'}`)
      break
    }

    case '/resume': {
      try {
        const sessions = await agent.listSessions(20)
        if (sessions.length === 0) {
          pushSystem(setSystem, 'info', 'No sessions found.')
          break
        }

        if (args.trim()) {
          const prefix = args.trim()
          const matches = sessions.filter(
            (s) => s.session_id === prefix || s.session_id.startsWith(prefix)
          )
          if (matches.length === 0) {
            pushSystem(setSystem, 'error', `Session not found: ${prefix}`)
          } else if (matches.length > 1) {
            pushSystem(setSystem, 'error', `Ambiguous session id: ${prefix}`)
          } else {
            const session = matches[0]!
            setState((prev) => ({
              ...prev,
              sessionId: session.session_id,
              messages: [],
            }))
            pushSystem(setSystem, 'info', `Resumed session ${session.session_id.slice(0, 8)} — ${session.title || '(untitled)'}`)
          }
        } else {
          const lines = sessions.slice(0, 10).map((s) => {
            const id = s.session_id.slice(0, 8)
            const title = s.title || '(untitled)'
            const time = relativeTime(s.updated_at)
            return `  ${id}  ${padRight(title, 40)}  ${time}`
          })
          pushSystem(setSystem, 'info', 'Recent sessions:\n' + lines.join('\n'))
          pushSystem(setSystem, 'info', 'Use /resume <id> to resume a session')
        }
      } catch (err: any) {
        pushSystem(setSystem, 'error', `Failed to list sessions: ${err?.message ?? err}`)
      }
      break
    }

    case '/plan':
      pushSystem(setSystem, 'info', 'Planning mode on — read-only tools only. Use /act to resume execution.')
      break

    case '/act':
      pushSystem(setSystem, 'info', 'Action mode on — full tool set restored.')
      break

    default:
      pushSystem(setSystem, 'error', `Unhandled command: ${name}`)
  }
}

// ---------------------------------------------------------------------------
// Async query runner
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
          /help for commands · Ctrl+L toggle verbose · Ctrl+C exit
        </Text>
      </Box>
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function relativeTime(iso: string): string {
  try {
    const ms = Date.now() - new Date(iso).getTime()
    const mins = Math.floor(ms / 60000)
    if (mins < 1) return 'just now'
    if (mins < 60) return `${mins}m ago`
    const hours = Math.floor(mins / 60)
    if (hours < 24) return `${hours}h ago`
    const days = Math.floor(hours / 24)
    return `${days}d ago`
  } catch {
    return iso
  }
}

function padRight(s: string, n: number): string {
  if (s.length > n) return s.slice(0, n - 1) + '…'
  return s + ' '.repeat(Math.max(0, n - s.length))
}
