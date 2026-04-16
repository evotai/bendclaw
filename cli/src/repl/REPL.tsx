/**
 * REPL screen — main interactive conversation view.
 * Orchestrates Agent queries, event streaming, and UI rendering.
 */

import React, { useState, useCallback, useRef, useEffect } from 'react'
import { Box, Text, useApp, useInput } from 'ink'
import { Agent, type RunEvent, QueryStream, version } from '../native/index.js'
import { type AppState, createInitialState } from '../state/app.js'
import { applyEvent } from '../state/reducer.js'
import type { UIMessage } from '../state/types.js'
import { PromptInput } from '../components/PromptInput.js'
import { OutputView } from '../components/OutputView.js'
import { ActiveResponse } from '../components/ActiveResponse.js'
import { HelpPane } from '../components/HelpPane.js'
import { ModelSelector } from '../components/ModelSelector.js'
import { SessionSelector } from '../components/SessionSelector.js'
import { HistoryManager } from '../session/history.js'
import { ScreenLog } from '../session/screen-log.js'
import { transcriptToMessages, type TranscriptItem } from '../session/transcript.js'
import { isSlashCommand } from '../commands/index.js'
import { handleSlashCommand } from '../commands/handle.js'
import { pushSystem, type SystemMsg } from './messages.js'
import { runLogTurn, runQuery } from './actions.js'
import { resumeSession, syncProvider } from './session.js'
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
} from '../render/output.js'
import { splitMarkdownBlocks } from '../render/markdown.js'
import { Banner } from './Banner.js'
import { UpdateManager, type ReleaseInfo } from '../update/index.js'
import { tryStartServer, setTerminalTitle, type ServerState } from './server.js'

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
  const [toolProgress, setToolProgress] = useState('')
  const [logMode, setLogMode] = useState<import('../native/index.js').ForkedAgent | null>(null)
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
  const [updateHint, setUpdateHint] = useState<string | undefined>(undefined)
  const [updateManager] = useState(() => new UpdateManager(version()))
  const [serverState, setServerState] = useState<ServerState | null>(null)
  const [configInfoState, setConfigInfoState] = useState(() => {
    try { return agent.configInfo() } catch { return undefined }
  })
  // Refresh configInfo when model changes (provider may have switched)
  useEffect(() => {
    try { setConfigInfoState(agent.configInfo()) } catch { /* ignore */ }
  }, [state.model])

  // Auto-check for updates
  useEffect(() => {
    const onUpdateAvailable = (info: ReleaseInfo) => {
      const changelogUrl = `https://github.com/evotai/evot/releases/tag/${info.tag}`
      const link = `\x1b]8;;${changelogUrl}\x1b\\${info.version}\x1b]8;;\x1b\\`
      setUpdateHint(`⬆ ${version()} → ${link} · /update`)
    }
    updateManager.on('update-available', onUpdateAvailable)
    updateManager.start()
    return () => {
      updateManager.off('update-available', onUpdateAvailable)
      updateManager.cleanup()
    }
  }, [])

  useEffect(() => {
    setTerminalTitle(null)
    tryStartServer().then((s) => {
      setServerState(s)
      setTerminalTitle(s)
    }).catch(() => {})
  }, [])

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
    runQuery(agent, text, sessionIdRef.current, streamRef, streamGenRef, setState, setOutputLines, setPendingText, setToolProgress, stateRef, planning ? 'planning' : undefined)
  }, [agent, planning])

  const handleSubmit = useCallback(
    (text: string) => {
      setSystemMessages([])

      // Log mode: /done exits, everything else goes to the forked agent
      if (logMode) {
        if (text.trim() === '/done') {
          setLogMode(null)
          pushSystem(setSystemMessages, 'info', '  [log mode ended]')
          return
        }
        // Run side conversation turn
        runLogTurn(logMode, text, setOutputLines, setPendingText, setState)
        return
      }

      if (isSlashCommand(text)) {
        handleSlashCommand(text, { agent, state: stateRef.current, setState, setSystem: setSystemMessages, setOutputLines, setPendingText, setShowHelp, setResumeSessions, setPlanning, setShowModelSelector, setLogMode, configInfo: configInfoState, abortCurrentStream, exit })
        return
      }

      if (isLoadingRef.current) {
        // Steer into the active run instead of queuing
        const stream = streamRef.current
        if (stream) {
          stream.steer(text)
          // Show the steered message in the UI immediately
          setOutputLines((prev) => [...prev, ...buildUserMessage(text)])
        } else {
          setMessageQueue((prev) => [...prev, text])
        }
        return
      }

      dispatchQuery(text)
    },
    [agent, exit, configInfoState, dispatchQuery, abortCurrentStream, logMode]
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
      setTerminalTitle(null)
      if (sessionIdRef.current) {
        console.log(`\n\x1b[90m${'─'.repeat(80)}\x1b[0m`)
        console.log(`\x1b[90mResume this session with:\n  evot --resume ${sessionIdRef.current}\x1b[0m\n`)
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
        banner={<Banner model={state.model} cwd={state.cwd} sessionId={state.sessionId} configInfo={configInfoState} serverState={serverState} />}
        lines={outputLines}
      />

      <ActiveResponse
        isLoading={state.isLoading}
        pendingText={pendingText}
        toolProgress={toolProgress}
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
        logMode={logMode !== null}
        queuedMessages={messageQueue}
        history={historyManager}
        updateHint={updateHint}
        serverState={serverState}
        onSubmit={handleSubmit}
        onInterrupt={handleInterrupt}
        onToggleVerbose={handleToggleVerbose}
      />
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

