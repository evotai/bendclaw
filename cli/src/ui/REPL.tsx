/**
 * REPL screen — main interactive conversation view.
 * Orchestrates Agent queries, event streaming, and UI rendering.
 */

import React, { useState, useCallback, useRef, useEffect } from 'react'
import { Box, Text, useApp, useInput } from 'ink'
import { Agent, type RunEvent, QueryStream } from '../native/index.js'
import { type AppState, createInitialState, applyEvent, type UIMessage } from '../state/AppState.js'
import { PromptInput } from '../components/PromptInput.js'
import { OutputView } from '../components/OutputView.js'
import { ActiveResponse } from '../components/ActiveResponse.js'
import { HelpPane } from '../components/HelpPane.js'
import { ModelSelector } from '../components/ModelSelector.js'
import { SessionSelector } from '../components/SessionSelector.js'
import { HistoryManager } from '../utils/history.js'
import { ScreenLog } from '../utils/screenLog.js'
import { transcriptToMessages, type TranscriptItem } from '../utils/transcript.js'
import { isSlashCommand, resolveCommand } from '../commands/index.js'
import { skillList, skillInstall, skillRemove } from '../commands/skill.js'
import {
  type OutputLine,
  buildUserMessage,
  buildAssistantLines,
  buildToolCall,
  buildToolResult,
  buildVerboseEvent,
  buildError,
  buildRunSummary,
  messagesToOutputLines,
  findSafeSplitPoint,
} from '../utils/outputLines.js'

interface REPLProps {
  agent: Agent
  initialVerbose?: boolean
  initialResume?: string
}

export function REPL({ agent, initialVerbose = true, initialResume }: REPLProps) {
  const { exit } = useApp()
  const [state, setState] = useState<AppState>(() => ({
    ...createInitialState(agent.model, agent.cwd),
    verbose: initialVerbose,
  }))
  const [systemMessages, setSystemMessages] = useState<SystemMsg[]>([])
  const [showHelp, setShowHelp] = useState(false)
  const [messageQueue, setMessageQueue] = useState<string[]>([])
  const [outputLines, setOutputLines] = useState<OutputLine[]>([])
  const [pendingText, setPendingText] = useState('')
  const [resumeSessions, setResumeSessions] = useState<import('../native/index.js').SessionMeta[] | null>(null)
  const [showModelSelector, setShowModelSelector] = useState(false)
  const [planning, setPlanning] = useState(false)
  const streamRef = useRef<QueryStream | null>(null)
  const sessionIdRef = useRef<string | null>(null)
  const isLoadingRef = useRef(false)
  const stateRef = useRef(state)
  stateRef.current = state
  const streamGenRef = useRef(0)  // generation counter to reject stale stream events
  const [historyManager] = useState(() => new HistoryManager())
  const [configInfoState, setConfigInfoState] = useState(() => {
    try { return agent.configInfo() } catch { return undefined }
  })
  // Refresh configInfo when model changes (provider may have switched)
  useEffect(() => {
    try { setConfigInfoState(agent.configInfo()) } catch { /* ignore */ }
  }, [state.model])

  // Startup: auto-resume or show resume hint
  useEffect(() => {
    (async () => {
      try {
        if (initialResume) {
          // Auto-resume from --resume flag
          const sessions = await agent.listSessions(20)
          const match = sessions.find((s) => s.session_id === initialResume || s.session_id.startsWith(initialResume))
          if (match) {
            await resumeSession(agent, match, setState, setSystemMessages, setOutputLines)
          } else {
            pushSystem(setSystemMessages, 'error', `Session not found: ${initialResume}`)
          }
        } else {
          const sessions = await agent.listSessions(20)
          const match = sessions.find((s) => s.cwd === agent.cwd)
          if (match) {
            pushSystem(setSystemMessages, 'info', `  previous session found. Use /resume ${match.session_id.slice(0, 8)} to continue.`)
          }
        }
      } catch { /* ignore */ }
    })()
  }, [])

  useEffect(() => {
    sessionIdRef.current = state.sessionId
  }, [state.sessionId])

  useEffect(() => {
    isLoadingRef.current = state.isLoading
  }, [state.isLoading])

  // Abort current stream and reset transient state
  const abortCurrentStream = useCallback(() => {
    const stream = streamRef.current
    if (stream) {
      streamRef.current = null
      streamGenRef.current++  // invalidate any in-flight event loop
      stream.abort()
    }
    setState((prev) => {
      // Commit any in-flight streaming text as an assistant message
      const messages = [...prev.messages]
      const text = prev.currentStreamText.trim()
      if (text.length > 0) {
        messages.push({
          id: `abort-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
          role: 'assistant',
          text,
          timestamp: Date.now(),
          toolCalls: prev.turnToolCalls.length > 0 ? prev.turnToolCalls : undefined,
          verboseEvents: prev.verboseEvents.length > 0 ? prev.verboseEvents : undefined,
        })
      }
      return {
        ...prev,
        messages,
        isLoading: false,
        currentStreamText: '',
        currentThinkingText: '',
        activeToolCalls: new Map(),
        turnToolCalls: [],
        verboseEvents: [],
      }
    })
  }, [])

  // Interrupt handler during loading — Ctrl+C or Escape
  useInput((_ch, key) => {
    const isInterrupt = (key.ctrl && _ch === 'c') || key.escape
    if (isInterrupt && streamRef.current) {
      abortCurrentStream()
      pushSystem(setSystemMessages, 'info', 'Interrupted.')
    }
  }, { isActive: state.isLoading })

  const dispatchQuery = useCallback((text: string) => {
    const userMsg: UIMessage = {
      id: `user-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
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
    runQuery(agent, text, sessionIdRef.current, streamRef, streamGenRef, setState, setOutputLines, setPendingText, stateRef, planning ? 'planning' : undefined)
  }, [agent, planning])

  const handleSubmit = useCallback(
    (text: string) => {
      setSystemMessages([])

      if (isSlashCommand(text)) {
        handleSlashCommand(text, { agent, state: stateRef.current, setState, setSystem: setSystemMessages, setOutputLines, setShowHelp, setResumeSessions, setPlanning, setShowModelSelector, configInfo: configInfoState, abortCurrentStream, exit })
        return
      }

      if (isLoadingRef.current) {
        setMessageQueue((prev) => [...prev, text])
        return
      }

      dispatchQuery(text)
    },
    [agent, exit, configInfoState, dispatchQuery, abortCurrentStream]
  )

  // Auto-drain queue when response finishes (skip if last run errored)
  useEffect(() => {
    if (!state.isLoading && !state.error && messageQueue.length > 0) {
      const [next, ...rest] = messageQueue
      setMessageQueue(rest)
      dispatchQuery(next!)
    }
  }, [state.isLoading, messageQueue, dispatchQuery])

  const handleInterrupt = useCallback(() => {
    if (streamRef.current) {
      abortCurrentStream()
      pushSystem(setSystemMessages, 'info', 'Interrupted.')
    } else {
      // Show resume hint on exit
      if (sessionIdRef.current) {
        console.log(`\n${'─'.repeat(80)}`)
        console.log(`Resume this session with:\n  evot --resume ${sessionIdRef.current}\n`)
      }
      exit()
    }
  }, [exit, abortCurrentStream])

  const handleToggleVerbose = useCallback(() => {
    setState((prev) => ({ ...prev, verbose: !prev.verbose }))
  }, [])

  return (
    <Box flexDirection="column" padding={0}>
      <OutputView
        banner={<Banner model={state.model} cwd={state.cwd} sessionId={state.sessionId} configInfo={configInfoState} />}
        lines={outputLines}
      />

      <ActiveResponse
        isLoading={state.isLoading}
        pendingText={pendingText}
        activeToolCalls={state.activeToolCalls}
        outputTokens={state.currentRunStats.outputTokens}
        lastTokenAt={state.lastTokenAt}
      />

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

      {showHelp && (
        <HelpPane onDismiss={() => setShowHelp(false)} />
      )}

      {resumeSessions !== null && (
        <SessionSelector
          sessions={resumeSessions}
          currentCwd={agent.cwd}
          onSelect={async (session) => {
            setResumeSessions(null)
            await resumeSession(agent, session, setState, setSystemMessages, setOutputLines)
          }}
          onCancel={() => setResumeSessions(null)}
        />
      )}

      {showModelSelector && (
        <ModelSelector
          models={configInfoState?.availableModels ?? [state.model]}
          currentModel={state.model}
          onSelect={(model) => {
            setShowModelSelector(false)
            agent.model = model
            syncProvider(agent, model, configInfoState)
            setState((prev) => ({ ...prev, model }))
            pushSystem(setSystemMessages, 'info', `Model → ${model}`)
          }}
          onCancel={() => setShowModelSelector(false)}
        />
      )}

      {/* Prompt input (Claude Code-style bordered box) */}
      <PromptInput
        model={state.model}
        isLoading={state.isLoading}
        isActive={!showHelp && resumeSessions === null && !showModelSelector}
        verbose={state.verbose}
        planning={planning}
        queuedMessages={messageQueue}
        history={historyManager}
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

interface CommandContext {
  agent: Agent
  state: AppState
  setState: React.Dispatch<React.SetStateAction<AppState>>
  setSystem: React.Dispatch<React.SetStateAction<SystemMsg[]>>
  setOutputLines: React.Dispatch<React.SetStateAction<OutputLine[]>>
  setShowHelp: React.Dispatch<React.SetStateAction<boolean>>
  setResumeSessions: React.Dispatch<React.SetStateAction<import('../native/index.js').SessionMeta[] | null>>
  setPlanning: React.Dispatch<React.SetStateAction<boolean>>
  setShowModelSelector: React.Dispatch<React.SetStateAction<boolean>>
  configInfo: import('../native/index.js').ConfigInfo | undefined
  abortCurrentStream: () => void
  exit: () => void
}

async function handleSlashCommand(input: string, ctx: CommandContext) {
  const { agent, state, setState, setSystem, setOutputLines, setShowHelp, setResumeSessions, setPlanning, setShowModelSelector, configInfo, abortCurrentStream, exit } = ctx
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
      setShowHelp(true)
      break

    case '/exit':
      abortCurrentStream()
      exit()
      break

    case '/clear':
      abortCurrentStream()
      setState((prev) => ({ ...prev, messages: [] }))
      setOutputLines([])
      pushSystem(setSystem, 'info', 'Messages cleared.')
      break

    case '/new':
      abortCurrentStream()
      setOutputLines([])
      setState((prev) => ({
        ...createInitialState(prev.model, prev.cwd),
        verbose: prev.verbose,
      }))
      pushSystem(setSystem, 'info', 'New session started.')
      break

    case '/model': {
      const arg = args.trim()
      if (arg === 'n') {
        // Cycle to next model
        const models = configInfo?.availableModels ?? [state.model]
        if (models.length <= 1) {
          pushSystem(setSystem, 'info', 'Only one model available.')
        } else {
          const idx = models.indexOf(state.model)
          const next = models[(idx + 1) % models.length]!
          agent.model = next
          syncProvider(agent, next, configInfo)
          setState((prev) => ({ ...prev, model: next }))
          pushSystem(setSystem, 'info', `Model → ${next}`)
        }
      } else if (arg) {
        agent.model = arg
        syncProvider(agent, arg, configInfo)
        setState((prev) => ({ ...prev, model: arg }))
        pushSystem(setSystem, 'info', `Model → ${arg}`)
      } else {
        setShowModelSelector(true)
      }
      break
    }

    case '/verbose':
      setState((prev) => ({ ...prev, verbose: !prev.verbose }))
      pushSystem(setSystem, 'info', `Verbose mode ${state.verbose ? 'off' : 'on'}`)
      break

    case '/resume': {
      abortCurrentStream()
      try {
        const allSessions = await agent.listSessions(20)
        if (allSessions.length === 0) {
          pushSystem(setSystem, 'info', 'No sessions found.')
          break
        }

        // Prefer sessions from current CWD
        const cwdSessions = allSessions.filter((s) => s.cwd === agent.cwd)
        const sessions = cwdSessions.length > 0 ? cwdSessions : allSessions

        if (args.trim()) {
          const prefix = args.trim()
          const matches = allSessions.filter(
            (s) => s.session_id === prefix || s.session_id.startsWith(prefix)
          )
          if (matches.length === 0) {
            pushSystem(setSystem, 'error', `Session not found: ${prefix}`)
          } else if (matches.length > 1) {
            pushSystem(setSystem, 'error', `Ambiguous session id: ${prefix}`)
          } else {
            const session = matches[0]!
            await resumeSession(agent, session, setState, setSystem, setOutputLines)
          }
        } else {
          // Show interactive session selector
          setResumeSessions(sessions.slice(0, 20))
        }
      } catch (err: any) {
        pushSystem(setSystem, 'error', `Failed to list sessions: ${err?.message ?? err}`)
      }
      break
    }

    case '/plan':
      setPlanning(true)
      pushSystem(setSystem, 'info', 'Planning mode on — read-only tools only. Use /act to resume execution.')
      break

    case '/act':
      setPlanning(false)
      pushSystem(setSystem, 'info', 'Action mode on — full tool set restored.')
      break

    case '/env': {
      const sub = args.trim()
      if (!sub) {
        // List agent variables + relevant process env
        const vars = agent.listVariables()
        if (vars.length > 0) {
          const lines = vars.map((v) => `  ${v.key.padEnd(28)} ${v.value.slice(0, 2)}****${v.value.slice(-2)}`)
          pushSystem(setSystem, 'info', `Agent variables:\n${lines.join('\n')}`)
        } else {
          pushSystem(setSystem, 'info', 'No agent variables set.')
        }
      } else if (sub.startsWith('set ')) {
        const kv = sub.slice(4).trim()
        const eqIdx = kv.indexOf('=')
        if (eqIdx <= 0) {
          pushSystem(setSystem, 'error', 'Usage: /env set KEY=VALUE')
        } else {
          const key = kv.slice(0, eqIdx)
          const val = kv.slice(eqIdx + 1)
          try {
            await agent.setVariable(key, val)
            pushSystem(setSystem, 'info', `Set ${key}`)
          } catch (err: any) {
            pushSystem(setSystem, 'error', `Failed: ${err?.message ?? err}`)
          }
        }
      } else if (sub.startsWith('del ')) {
        const key = sub.slice(4).trim()
        if (!key) {
          pushSystem(setSystem, 'error', 'Usage: /env del KEY')
        } else {
          try {
            const removed = await agent.deleteVariable(key)
            pushSystem(setSystem, 'info', removed ? `Deleted ${key}` : `${key} not found`)
          } catch (err: any) {
            pushSystem(setSystem, 'error', `Failed: ${err?.message ?? err}`)
          }
        }
      } else if (sub.startsWith('load ')) {
        const filePath = sub.slice(5).trim()
        try {
          const { readFileSync } = await import('fs')
          const content = readFileSync(filePath, 'utf-8')
          let count = 0
          for (const line of content.split('\n')) {
            const trimmed = line.trim()
            if (!trimmed || trimmed.startsWith('#')) continue
            const clean = trimmed.startsWith('export ') ? trimmed.slice(7) : trimmed
            const eq = clean.indexOf('=')
            if (eq > 0) {
              const k = clean.slice(0, eq)
              let v = clean.slice(eq + 1)
              if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) {
                v = v.slice(1, -1)
              }
              await agent.setVariable(k, v)
              count++
            }
          }
          pushSystem(setSystem, 'info', `Loaded ${count} vars from ${filePath}`)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `Failed to load: ${err?.message ?? err}`)
        }
      } else {
        pushSystem(setSystem, 'error', 'Usage: /env [set K=V | del K | load FILE]')
      }
      break
    }

    case '/log': {
      const { join } = await import('path')
      const { homedir } = await import('os')
      const logDir = join(homedir(), '.evotai', 'logs')
      const sid = state.sessionId
      const query = args.trim()

      if (!query) {
        // Just show log path
        if (sid) {
          pushSystem(setSystem, 'info', `Log: ${join(logDir, `${sid}.screen.log`)}`)
        } else {
          pushSystem(setSystem, 'info', `Log dir: ${logDir} (no active session)`)
        }
      } else if (!sid) {
        pushSystem(setSystem, 'error', 'No active session to analyze.')
      } else {
        // Side conversation: fork agent to analyze the log
        const logPath = join(logDir, `${sid}.screen.log`)
        const systemPrompt = [
          'You are in a temporary log analysis session.',
          `The session log file is at: ${logPath}`,
          'Read the log file before answering any questions.',
          'Do not modify any files. Only read and analyze.',
          'Keep answers concise and focused on the log content.',
        ].join('\n')

        try {
          const forked = agent.fork(systemPrompt)
          pushSystem(setSystem, 'info', `Analyzing log...`)

          // Run single-turn analysis (no multi-turn mini-REPL)
          const stream = await forked.query(query)
          let text = ''
          for await (const event of stream) {
            if (event.kind === 'assistant_delta' && event.payload?.delta) {
              text += event.payload.delta as string
            }
          }
          if (text) pushSystem(setSystem, 'info', text)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `Fork failed: ${err?.message ?? err}`)
        }
      }
      break
    }

    case '/skill': {
      const sub = args.trim()
      if (!sub || sub === 'list') {
        pushSystem(setSystem, 'info', skillList())
      } else if (sub.startsWith('install ')) {
        const source = sub.slice(8).trim()
        if (!source) {
          pushSystem(setSystem, 'error', 'Usage: /skill install <owner/repo>')
          break
        }
        pushSystem(setSystem, 'info', `  cloning ${source}`)
        try {
          const forked = agent.fork('You analyze skills and provide setup guides.')
          const result = await skillInstall(source, forked, (msg, level) => {
            pushSystem(setSystem, level, msg)
          })
          if (result) pushSystem(setSystem, 'info', result)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `  install failed: ${err?.message ?? err}`)
        }
      } else if (sub.startsWith('remove ')) {
        const name = sub.slice(7).trim()
        if (!name) {
          pushSystem(setSystem, 'error', 'Usage: /skill remove <name>')
        } else {
          pushSystem(setSystem, 'info', skillRemove(name))
        }
      } else {
        pushSystem(setSystem, 'error', 'Usage: /skill [list | install <source> | remove <name>]')
      }
      break
    }

    default:
      pushSystem(setSystem, 'error', `Unhandled command: ${name}`)
  }
}

// ---------------------------------------------------------------------------
// Resume session helper
// ---------------------------------------------------------------------------

async function resumeSession(
  agent: Agent,
  session: import('../native/index.js').SessionMeta,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  setSystem: React.Dispatch<React.SetStateAction<SystemMsg[]>>,
  setOutputLines: React.Dispatch<React.SetStateAction<OutputLine[]>>,
) {
  let messages: UIMessage[] = []
  try {
    const transcript = await agent.loadTranscript(session.session_id)
    messages = transcriptToMessages(transcript as TranscriptItem[])
  } catch { /* ignore */ }

  // Restore model from session
  if (session.model) {
    agent.model = session.model
    syncProvider(agent, session.model)
  }

  // Convert messages to OutputLines so they appear in <Static>
  const lines = messagesToOutputLines(messages)
  setOutputLines(lines)

  setState((prev) => ({
    ...createInitialState(session.model || prev.model, prev.cwd),
    verbose: prev.verbose,
    sessionId: session.session_id,
    messages,
  }))
  pushSystem(setSystem, 'info', `Resumed session ${session.session_id.slice(0, 8)} — ${session.title || '(untitled)'}`)
}

// ---------------------------------------------------------------------------
// Provider sync — infer provider from model name and switch if needed
// ---------------------------------------------------------------------------

function syncProvider(
  agent: Agent,
  model: string,
  configInfo?: import('../native/index.js').ConfigInfo,
): void {
  try {
    // First try exact match against configured models
    if (configInfo) {
      if (model === configInfo.anthropicModel) { agent.setProvider('anthropic'); return }
      if (model === configInfo.openaiModel) { agent.setProvider('openai'); return }
    }
    // Fall back to prefix heuristic
    if (model.startsWith('claude-') || model.startsWith('anthropic/')) {
      agent.setProvider('anthropic')
    } else if (model.startsWith('gpt-') || model.startsWith('o1-') || model.startsWith('o3-') || model === 'o1' || model === 'o3') {
      agent.setProvider('openai')
    }
  } catch { /* ignore — provider may not support the model */ }
}

// ---------------------------------------------------------------------------
// Async query runner
// ---------------------------------------------------------------------------

async function runQuery(
  agent: Agent,
  text: string,
  sessionId: string | null,
  streamRef: React.MutableRefObject<QueryStream | null>,
  streamGenRef: React.MutableRefObject<number>,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  setOutputLines: React.Dispatch<React.SetStateAction<OutputLine[]>>,
  setPendingText: React.Dispatch<React.SetStateAction<string>>,
  stateRef: React.MutableRefObject<AppState>,
  toolMode?: string,
) {
  const gen = ++streamGenRef.current  // claim a new generation
  let streamingText = ''  // accumulates all assistant text for current turn
  let prefixEmitted = false
  let localState = stateRef.current  // local copy, updated synchronously

  // Helper: append OutputLines to Static and write them to the screen log
  let screenLog: ScreenLog | null = null
  const appendLines = (lines: OutputLine[]) => {
    if (lines.length === 0) return
    setOutputLines((prev) => [...prev, ...lines])
    try { screenLog?.writeLines(lines) } catch { /* ignore */ }
  }

  // Commit accumulated streaming text to Static
  const commitStreamingText = () => {
    if (!streamingText.trim()) {
      streamingText = ''
      setPendingText('')
      return
    }
    const needsPrefix = !prefixEmitted
    const lines = buildAssistantLines(streamingText, needsPrefix)
    if (needsPrefix && lines.length > 0) prefixEmitted = true
    appendLines(lines)
    streamingText = ''
    setPendingText('')
  }

  try {
    const stream = await agent.query(text, sessionId ?? undefined, toolMode)
    // If generation changed while awaiting, another command took over — bail
    if (gen !== streamGenRef.current) { stream.abort(); return }
    streamRef.current = stream

    // Start screen log for this session
    try {
      screenLog = new ScreenLog(stream.sessionId)
    } catch { /* ignore log failures */ }

    // Show user message on screen and log it
    const userLines = buildUserMessage(text)
    appendLines(userLines)

    for await (const event of stream) {
      if (gen !== streamGenRef.current) break  // stale — stop processing

      const p = event.payload as Record<string, any>

      // Compute next state synchronously for accurate verbose/stats data
      const nextState = applyEvent(localState, event)

      // --- Verbose events that appear BEFORE content ---
      if (localState.verbose
        && (event.kind === 'llm_call_started' || event.kind === 'context_compaction_started')) {
        commitStreamingText()
        const newEvents = nextState.verboseEvents.slice(localState.verboseEvents.length)
        for (const evt of newEvents) {
          appendLines(buildVerboseEvent(evt.text))
        }
      }

      // --- Streaming assistant text (dynamic zone) ---
      if (event.kind === 'assistant_delta') {
        const delta = p.delta as string | undefined
        if (delta) {
          streamingText += delta
          if (!prefixEmitted) {
            const trimmed = streamingText.replace(/^[\n\r]+/, '')
            if (trimmed.length > 0) streamingText = trimmed
          }
          // Auto-commit when dynamic zone gets too tall
          const termRows = process.stdout.rows ?? 24
          if (streamingText.split('\n').length > termRows - 8) {
            const splitAt = findSafeSplitPoint(streamingText)
            if (splitAt > 0 && splitAt < streamingText.length) {
              const completed = streamingText.slice(0, splitAt)
              streamingText = streamingText.slice(splitAt)
              const needsPrefix = !prefixEmitted
              const lines = buildAssistantLines(completed, needsPrefix)
              if (needsPrefix && lines.length > 0) prefixEmitted = true
              appendLines(lines)
            }
          }
          setPendingText(streamingText)
        }
      }

      // --- Turn boundaries: commit pending text to Static ---
      if (event.kind === 'assistant_completed' || event.kind === 'turn_started') {
        commitStreamingText()
      }

      // --- Verbose events that appear AFTER content ---
      if (localState.verbose
        && (event.kind === 'llm_call_completed' || event.kind === 'context_compaction_completed')) {
        commitStreamingText()
        const newEvents = nextState.verboseEvents.slice(localState.verboseEvents.length)
        for (const evt of newEvents) {
          appendLines(buildVerboseEvent(evt.text))
        }
      }

      // --- Tool started: show tool call badge with preview command ---
      if (event.kind === 'tool_started') {
        commitStreamingText()
        const toolName = p.tool_name ?? 'unknown'
        const args = p.args ?? {}
        const previewCommand = p.preview_command as string | undefined
        appendLines(buildToolCall(toolName, args, previewCommand))
      }

      // --- Tool finished: show result ---
      if (event.kind === 'tool_finished') {
        const toolName = p.tool_name ?? 'unknown'
        const args = p.args ?? {}
        const details = p.details as Record<string, any> | undefined
        const mergedArgs = details?.diff ? { ...args, diff: details.diff } : args
        const status = p.is_error ? 'error' as const : 'done' as const
        appendLines(buildToolResult(toolName, mergedArgs, status, p.content, p.duration_ms))
      }

      // --- Run summary on finish ---
      if (event.kind === 'run_finished' && localState.verbose) {
        commitStreamingText()
        const stats = nextState.currentRunStats
        appendLines(buildRunSummary(stats))
      }

      // Advance local state and push to React
      localState = nextState
      setState(() => localState)
    }

    // Flush any remaining content
    commitStreamingText()
  } catch (err: any) {
    commitStreamingText()

    if (gen !== streamGenRef.current) return  // stale — don't overwrite new session's state
    const errLines = buildError(err?.message ?? String(err))
    appendLines(errLines)
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

function Banner({ model, cwd, sessionId, configInfo }: {
  model: string
  cwd: string
  sessionId: string | null
  configInfo?: import('../native/index.js').ConfigInfo
}) {
  const shortCwd = cwd.replace(process.env.HOME ?? '', '~')
  const gitBranch = getGitBranch(cwd)
  const sessionLabel = sessionId ? sessionId.slice(0, 8) : '(new)'
  const envPath = configInfo?.envPath?.replace(process.env.HOME ?? '', '~') ?? ''

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box>
        <Text backgroundColor="blue" color="white" bold>
          {' ◆ evot '}
        </Text>
        <Text dimColor> v0.1.0</Text>
      </Box>
      {envPath ? <Text dimColor>  env:      {envPath}</Text> : null}
      {configInfo && <Text dimColor>  provider: {configInfo.provider}</Text>}
      <Text dimColor>  model:    {model}</Text>
      {configInfo?.baseUrl && <Text dimColor>  base_url: {configInfo.baseUrl}</Text>}
      <Text dimColor>  session:  {sessionLabel}</Text>
      {gitBranch && <Text dimColor>  git:      {gitBranch}</Text>}
      <Text dimColor>  cwd:      {shortCwd}</Text>
      <Text dimColor>  /help commands  ·  Tab complete  ·  ↑↓ history  ·  Ctrl+C×2 exit</Text>
    </Box>
  )
}

function getGitBranch(cwd: string): string | null {
  try {
    const result = Bun.spawnSync(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], {
      cwd,
      stdout: 'pipe',
      stderr: 'pipe',
    })
    if (result.exitCode === 0) {
      return result.stdout.toString().trim()
    }
  } catch { /* not a git repo */ }
  return null
}
