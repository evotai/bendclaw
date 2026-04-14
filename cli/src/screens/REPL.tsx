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
import { HelpPane } from '../components/HelpPane.js'
import { ModelSelector } from '../components/ModelSelector.js'
import { SessionSelector } from '../components/SessionSelector.js'
import { HistoryManager } from '../utils/history.js'
import { TranscriptLog } from '../utils/transcriptLog.js'
import { transcriptToMessages, type TranscriptItem } from '../utils/transcript.js'
import { isSlashCommand, resolveCommand } from '../commands/index.js'
import { skillList, skillInstall, skillRemove } from '../commands/skill.js'

interface REPLProps {
  agent: Agent
}

export function REPL({ agent }: REPLProps) {
  const { exit } = useApp()
  const [state, setState] = useState<AppState>(() =>
    createInitialState(agent.model, agent.cwd)
  )
  const [systemMessages, setSystemMessages] = useState<SystemMsg[]>([])
  const [showHelp, setShowHelp] = useState(false)
  const [messageQueue, setMessageQueue] = useState<string[]>([])
  const [resumeSessions, setResumeSessions] = useState<import('../native/index.js').SessionMeta[] | null>(null)
  const [showModelSelector, setShowModelSelector] = useState(false)
  const [planning, setPlanning] = useState(false)
  const streamRef = useRef<QueryStream | null>(null)
  const sessionIdRef = useRef<string | null>(null)
  const [historyManager] = useState(() => new HistoryManager())
  const [cachedConfigInfo] = useState(() => {
    try { return agent.configInfo() } catch { return undefined }
  })

  // Startup: show resume hint for most recent CWD-matching session
  useEffect(() => {
    (async () => {
      try {
        const sessions = await agent.listSessions(20)
        const match = sessions.find((s) => s.cwd === agent.cwd)
        if (match) {
          pushSystem(setSystemMessages, 'info', `  previous session found. Use /resume ${match.session_id.slice(0, 8)} to continue.`)
        }
      } catch { /* ignore */ }
    })()
  }, [])

  useEffect(() => {
    sessionIdRef.current = state.sessionId
  }, [state.sessionId])

  // Interrupt handler during loading — Ctrl+C or Escape
  useInput((_ch, key) => {
    const isInterrupt = (key.ctrl && _ch === 'c') || key.escape
    if (isInterrupt && streamRef.current) {
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
  }, { isActive: state.isLoading })

  const handleSubmit = useCallback(
    (text: string) => {
      setSystemMessages([])

      if (isSlashCommand(text)) {
        handleSlashCommand(text, agent, state, setState, setSystemMessages, setShowHelp, setResumeSessions, setPlanning, setShowModelSelector, exit)
        return
      }

      // If loading, queue the message instead of running immediately
      if (state.isLoading) {
        setMessageQueue((prev) => [...prev, text])
        return
      }

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

      runQuery(agent, text, sessionIdRef.current, streamRef, setState, planning ? 'planning' : undefined)
    },
    [agent, state, exit, planning]
  )

  // Auto-drain queue when response finishes (skip if last run errored)
  useEffect(() => {
    if (!state.isLoading && !state.error && messageQueue.length > 0) {
      const [next, ...rest] = messageQueue
      setMessageQueue(rest)
      const userMsg: UIMessage = {
        id: `user-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        role: 'user',
        text: next!,
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
      runQuery(agent, next!, sessionIdRef.current, streamRef, setState, planning ? 'planning' : undefined)
    }
  }, [state.isLoading, messageQueue, agent])

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
      // Show resume hint on exit
      if (sessionIdRef.current) {
        console.log(`\n${'─'.repeat(80)}`)
        console.log(`Resume this session with:\n  evot --resume ${sessionIdRef.current}\n`)
      }
      exit()
    }
  }, [exit])

  const handleToggleVerbose = useCallback(() => {
    setState((prev) => ({ ...prev, verbose: !prev.verbose }))
  }, [])

  const hasStreamText = state.currentStreamText.length > 0
  const hasThinkingText = state.currentThinkingText.length > 0
  const hasActiveTools = state.activeToolCalls.size > 0

  return (
    <Box flexDirection="column" padding={0}>
      <Banner model={state.model} cwd={state.cwd} sessionId={state.sessionId} configInfo={cachedConfigInfo} />

      {/* Message history with interleaved verbose events */}
      {state.messages.map((msg) => (
        <React.Fragment key={msg.id}>
          {msg.verboseEvents?.map((evt, i) => (
            <VerboseEventLine key={`${msg.id}-evt-${i}`} event={evt} />
          ))}
          <Message key={msg.id} message={msg} />
          {state.verbose && msg.runStats && (
            <RunSummary stats={msg.runStats} />
          )}
        </React.Fragment>
      ))}

      {/* Pending verbose events for current turn (not yet attached to a message) */}
      {state.isLoading && state.verboseEvents.length > 0 && (
        <Box flexDirection="column" marginBottom={0}>
          {state.verboseEvents.map((evt, i) => (
            <VerboseEventLine key={i} event={evt} />
          ))}
        </Box>
      )}

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
      {state.isLoading && !hasStreamText && !hasThinkingText && (
        <Spinner
          toolName={hasActiveTools ? [...state.activeToolCalls.values()][0]?.name : undefined}
          progressText={hasActiveTools ? [...state.activeToolCalls.values()][0]?.previewCommand : undefined}
          tokenCount={state.currentRunStats.outputTokens}
        />
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

      {/* Help overlay */}
      {showHelp && (
        <HelpPane onDismiss={() => setShowHelp(false)} />
      )}

      {/* Session selector for /resume */}
      {resumeSessions !== null && (
        <SessionSelector
          sessions={resumeSessions}
          currentCwd={agent.cwd}
          onSelect={async (session) => {
            setResumeSessions(null)
            await resumeSession(agent, session, setState, setSystemMessages)
          }}
          onCancel={() => setResumeSessions(null)}
        />
      )}

      {/* Model selector for /model */}
      {showModelSelector && (
        <ModelSelector
          models={[...getAvailableModels(agent), ...FALLBACK_MODELS.filter((m) => !getAvailableModels(agent).includes(m))]}
          currentModel={state.model}
          onSelect={(model) => {
            setShowModelSelector(false)
            agent.model = model
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

async function handleSlashCommand(
  input: string,
  agent: Agent,
  state: AppState,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  setSystem: React.Dispatch<React.SetStateAction<SystemMsg[]>>,
  setShowHelp: React.Dispatch<React.SetStateAction<boolean>>,
  setResumeSessions: React.Dispatch<React.SetStateAction<import('../native/index.js').SessionMeta[] | null>>,
  setPlanning: React.Dispatch<React.SetStateAction<boolean>>,
  setShowModelSelector: React.Dispatch<React.SetStateAction<boolean>>,
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
      setShowHelp(true)
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
      const arg = args.trim()
      if (arg === 'n') {
        // Cycle to next model
        const models = getAvailableModels(agent)
        if (models.length <= 1) {
          pushSystem(setSystem, 'info', 'Only one model available.')
        } else {
          const idx = models.indexOf(state.model)
          const next = models[(idx + 1) % models.length]!
          agent.model = next
          // Auto-switch provider if needed
          const config = agent.configInfo()
          if (next === config.anthropicModel && next !== config.openaiModel) {
            try { agent.setProvider('anthropic') } catch {}
          } else if (next === config.openaiModel && next !== config.anthropicModel) {
            try { agent.setProvider('openai') } catch {}
          }
          setState((prev) => ({ ...prev, model: next }))
          pushSystem(setSystem, 'info', `Model → ${next}`)
        }
      } else if (arg) {
        agent.model = arg
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
            await resumeSession(agent, session, setState, setSystem)
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
          pushSystem(setSystem, 'info', `Log: ${join(logDir, `${sid}.log`)}`)
        } else {
          pushSystem(setSystem, 'info', `Log dir: ${logDir} (no active session)`)
        }
      } else if (!sid) {
        pushSystem(setSystem, 'error', 'No active session to analyze.')
      } else {
        // Side conversation: fork agent to analyze the log
        const logPath = join(logDir, `${sid}.log`)
        const systemPrompt = [
          'You are in a temporary log analysis session.',
          `The session log file is at: ${logPath}`,
          'Read the log file before answering any questions.',
          'Do not modify any files. Only read and analyze.',
          'Keep answers concise and focused on the log content.',
        ].join('\n')

        try {
          const forked = agent.fork(systemPrompt)
          pushSystem(setSystem, 'info', `Analyzing log... (type /done to exit)`)

          // Run first turn
          const stream = await forked.query(query)
          let text = ''
          for await (const event of stream) {
            if (event.kind === 'assistant_delta' && event.payload?.delta) {
              text += event.payload.delta as string
            }
          }
          if (text) pushSystem(setSystem, 'info', text)

          // Enter mini-REPL loop via state — store forked agent for subsequent turns
          setState((prev) => ({ ...prev, forkedAgent: forked } as any))
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
        pushSystem(setSystem, 'info', `Installing skill from ${source}...`)
        try {
          const forked = agent.fork('You analyze skills and provide setup guides.')
          const result = await skillInstall(source, forked)
          pushSystem(setSystem, 'info', result)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `Install failed: ${err?.message ?? err}`)
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
// VerboseEventLine — colored badges for [COMPACT] and [LLM] events
// ---------------------------------------------------------------------------

function VerboseEventLine({ event }: { event: import('../state/AppState.js').VerboseEvent }) {
  const lines = event.text.split('\n')
  const isCompact = event.kind === 'compact_call' || event.kind === 'compact_done'
  const isLlm = event.kind === 'llm_call' || event.kind === 'llm_completed'

  // First line has the badge, rest are indented detail lines
  const [firstLine, ...rest] = lines

  // Extract badge and remainder from first line: "[COMPACT] call" or "[LLM] completed"
  const badgeMatch = firstLine?.match(/^\[(\w+)\]\s*(.*)$/)
  const badge = badgeMatch ? badgeMatch[1] : ''
  const after = badgeMatch ? badgeMatch[2] : firstLine ?? ''

  return (
    <Box flexDirection="column" marginTop={1}>
      <Box>
        {isCompact && <Text color="green" bold>[{badge}]</Text>}
        {isLlm && <Text color="yellow" bold>[{badge}]</Text>}
        {after ? <Text dimColor> {after}</Text> : null}
      </Box>
      {rest.map((line, i) => (
        <Box key={i}>
          <Text dimColor>{line}</Text>
        </Box>
      ))}
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Resume session helper
// ---------------------------------------------------------------------------

async function resumeSession(
  agent: Agent,
  session: import('../native/index.js').SessionMeta,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  setSystem: React.Dispatch<React.SetStateAction<SystemMsg[]>>,
) {
  let messages: UIMessage[] = []
  try {
    const transcript = await agent.loadTranscript(session.session_id)
    messages = transcriptToMessages(transcript as TranscriptItem[])
  } catch { /* ignore */ }

  // Restore model from session
  if (session.model) {
    agent.model = session.model
    // Auto-switch provider
    const config = agent.configInfo()
    if (session.model === config.anthropicModel) {
      try { agent.setProvider('anthropic') } catch {}
    } else if (session.model === config.openaiModel) {
      try { agent.setProvider('openai') } catch {}
    }
  }

  setState((prev) => ({
    ...prev,
    sessionId: session.session_id,
    model: session.model || prev.model,
    messages,
  }))
  pushSystem(setSystem, 'info', `Resumed session ${session.session_id.slice(0, 8)} — ${session.title || '(untitled)'}`)
}

// ---------------------------------------------------------------------------
// Available models (from config + fallback list)
// ---------------------------------------------------------------------------

function getAvailableModels(agent: Agent): string[] {
  const config = agent.configInfo()
  const models: string[] = []
  for (const m of [config.anthropicModel, config.openaiModel, agent.model]) {
    if (m && m.trim() && !models.includes(m)) models.push(m)
  }
  return models
}

const FALLBACK_MODELS = [
  'claude-opus-4-6',
  'claude-sonnet-4-6',
  'claude-haiku-4-5-20251001',
  'gpt-4o',
  'gpt-4o-mini',
  'o3',
  'o3-mini',
  'gemini-2.5-pro',
  'gemini-2.5-flash',
]

// ---------------------------------------------------------------------------
// Async query runner
// ---------------------------------------------------------------------------

async function runQuery(
  agent: Agent,
  text: string,
  sessionId: string | null,
  streamRef: React.MutableRefObject<QueryStream | null>,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  toolMode?: string,
) {
  try {
    const stream = await agent.query(text, sessionId ?? undefined, toolMode)
    streamRef.current = stream

    // Start transcript log for this session
    let log: TranscriptLog | null = null
    try {
      log = new TranscriptLog(stream.sessionId)
      log.writeUserPrompt(text)
    } catch { /* ignore log failures */ }

    for await (const event of stream) {
      setState((prev) => applyEvent(prev, event))
      try { log?.writeEvent(event) } catch { /* ignore */ }
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
        <Text backgroundColor="#5a2d82" color="white" bold>
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
