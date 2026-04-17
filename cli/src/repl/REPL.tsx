/**
 * REPL screen — main interactive conversation view.
 * Orchestrates Agent queries, event streaming, and UI rendering.
 */

import React, { useState, useCallback, useRef, useEffect } from 'react'
import { Box, Text, useApp } from 'ink'
import { Agent, type RunEvent, QueryStream, version } from '../native/index.js'
import { type AppState, createInitialState } from '../state/app.js'
import { applyEvent } from '../state/reducer.js'
import type { UIMessage } from '../state/types.js'
import { PromptInput } from '../components/PromptInput.js'
import type { PromptPayload, PastedImage } from '../components/PromptInput.js'
import { OutputView } from '../components/OutputView.js'
import { ActiveResponse } from '../components/ActiveResponse.js'
import { HelpPane } from '../components/HelpPane.js'
import { ModelSelector } from '../components/ModelSelector.js'
import { SessionSelector } from '../components/SessionSelector.js'
import { AskUser } from '../components/AskUser.js'
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
import { Banner, type BannerProps } from './Banner.js'
import { UpdateManager, type ReleaseInfo } from '../update/index.js'
import { tryStartServer, setTerminalTitle, type ServerState } from './server.js'

interface REPLProps {
  agent: Agent
  initialVerbose?: boolean
  initialResume?: string
  preloadedSessions?: import('../native/index.js').SessionMeta[]
  preloadedReleaseNotes?: string[]
  onEmptyPaste?: (handler: () => void) => void
}

export function REPL({ agent, initialVerbose = true, initialResume, preloadedSessions, preloadedReleaseNotes, onEmptyPaste }: REPLProps) {
  const { exit } = useApp()
  const [state, setState] = useState<AppState>(() => ({
    ...createInitialState(agent.model, agent.cwd),
    verbose: initialVerbose,
  }))
  const [systemMessages, setSystemMessages] = useState<SystemMsg[]>([])
  const [showHelp, setShowHelp] = useState(false)
  const [messageQueue, setMessageQueue] = useState<{ text: string; images?: PastedImage[] }[]>([])
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
  const [recentSessions] = useState<import('../native/index.js').SessionMeta[]>(preloadedSessions ?? [])
  const [releaseNotes] = useState<string[]>(preloadedReleaseNotes ?? [])
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
    (async () => {
      setTerminalTitle()
      try {
        const s = await tryStartServer()
        setServerState(s)
        setTerminalTitle()
        if (s) {
          const parts = [`  server: ${s.address}`]
          if (s.channels.length > 0) parts.push(`channels: ${s.channels.join(', ')}`)
          pushSystem(setSystemMessages, 'info', parts.join('  ·  '))
        }
      } catch { /* ignore */ }

      try {
        if (initialResume) {
          const sessions = preloadedSessions ?? await agent.listSessions(20)
          const match = sessions.find((s) => s.session_id === initialResume || s.session_id.startsWith(initialResume))
          if (match) {
            await resumeSession(agent, match, setState, setSystemMessages, setOutputLines)
          } else {
            pushSystem(setSystemMessages, 'error', `Session not found: ${initialResume}`)
          }
        } else {
          const sessions = preloadedSessions ?? await agent.listSessions(20)
          const match = sessions.find((s) => s.cwd === agent.cwd)
          if (match) {
            const tag = match.source ? `[${match.source}] ` : ''
            const title = match.title || '(untitled)'
            const short = title.length > 40 ? title.slice(0, 39) + '…' : title
            pushSystem(setSystemMessages, 'info', `  previous session: ${tag}${short} · /resume ${match.session_id.slice(0, 8)}`)
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

  const dispatchQuery = useCallback((text: string, images?: PastedImage[]) => {
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
    runQuery(agent, text, images, sessionIdRef.current, streamRef, streamGenRef, setState, setOutputLines, setPendingText, setToolProgress, stateRef, planning ? 'planning_interactive' : 'interactive')
  }, [agent, planning])

  const handleSubmit = useCallback(
    (payload: PromptPayload) => {
      const { text, images } = payload
      setSystemMessages([])

      // Log mode: /done exits, everything else goes to the forked agent
      if (logMode) {
        if (images.length > 0) {
          pushSystem(setSystemMessages, 'warn', '  Images are not supported in log mode — ignored')
        }
        if (text.trim() === '/done') {
          setLogMode(null)
          pushSystem(setSystemMessages, 'info', '  [log mode ended]')
          return
        }
        // Run side conversation turn
        runLogTurn(logMode, text, setOutputLines, setPendingText, setState)
        return
      }

      // Slash commands don't support images
      if (isSlashCommand(text)) {
        if (images.length > 0) {
          pushSystem(setSystemMessages, 'warn', '  Images are not supported with slash commands — ignored')
        }
        handleSlashCommand(text, { agent, state: stateRef.current, setState, setSystem: setSystemMessages, setOutputLines, setPendingText, setShowHelp, setResumeSessions, setPlanning, setShowModelSelector, setLogMode, configInfo: configInfoState, abortCurrentStream, exit })
        return
      }

      if (isLoadingRef.current) {
        // Steer into the active run — supports text + images
        const stream = streamRef.current
        if (stream) {
          if (images.length > 0) {
            const content: import('../native/index.js').ContentBlock[] = [
              ...(text ? [{ type: 'text' as const, text }] : []),
              ...images.map((img) => ({
                type: 'image' as const,
                data: img.base64,
                mimeType: img.mediaType,
              })),
            ]
            stream.steer('', JSON.stringify(content))
          } else {
            stream.steer(text)
          }
          // Show the steered message in the UI immediately
          setOutputLines((prev) => [...prev, ...buildUserMessage(text, images.length > 0 ? images.length : undefined)])
        } else {
          setMessageQueue((prev) => [...prev, { text, images: images.length > 0 ? images : undefined }])
        }
        return
      }

      dispatchQuery(text, images.length > 0 ? images : undefined)
    },
    [agent, exit, configInfoState, dispatchQuery, abortCurrentStream, logMode]
  )

  // Auto-drain queue when response finishes (skip if last run errored)
  useEffect(() => {
    if (!state.isLoading && !state.error && messageQueue.length > 0) {
      const [next, ...rest] = messageQueue
      setMessageQueue(rest)
      dispatchQuery(next!.text, next!.images)
    }
  }, [state.isLoading, messageQueue, dispatchQuery])

  const handleInterrupt = useCallback(() => {
    if (streamRef.current) {
      abortCurrentStream()
      pushSystem(setSystemMessages, 'info', 'Interrupted.')
    } else {
      setTerminalTitle()
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
        banner={<Banner model={state.model} cwd={state.cwd} sessionId={state.sessionId} configInfo={configInfoState} recentSessions={recentSessions} releaseNotes={releaseNotes} />}
        lines={outputLines}
      />

      {!state.askUserRequest && (
        <ActiveResponse
          isLoading={state.isLoading}
          pendingText={pendingText}
          toolProgress={toolProgress}
          activeToolCalls={state.activeToolCalls}
          outputTokens={state.currentRunStats.outputTokens}
          lastTokenAt={state.lastTokenAt}
        />
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

      {state.askUserRequest && (
        <AskUser
          request={state.askUserRequest}
          onSubmit={(answers) => {
            const stream = streamRef.current
            const questions = state.askUserRequest!.questions
            // Map component answers to the Rust AskUserResponse format
            const rustAnswers = answers.map((a) => ({
              header: questions[a.questionIndex]?.header ?? '',
              question: questions[a.questionIndex]?.question ?? '',
              answer: a.customText ?? questions[a.questionIndex]?.options[a.selectedOption ?? 0]?.label ?? '',
            }))
            // Show answer summary in output
            const summaryLines: import('../render/output.js').OutputLine[] = [
              { id: `ask-${Date.now()}`, kind: 'tool_result', text: '● User answered:' },
              ...rustAnswers.map((a, i) => ({
                id: `ask-a-${Date.now()}-${i}`,
                kind: 'tool_result' as const,
                text: `  · ${a.question} → ${a.answer}`,
              })),
            ]
            setOutputLines((prev) => [...prev, ...summaryLines])
            if (stream) {
              const json = JSON.stringify({ Answered: rustAnswers })
              stream.respondAskUser(json)
            }
            setState((prev) => ({ ...prev, askUserRequest: null }))
          }}
          onCancel={() => {
            const stream = streamRef.current
            setOutputLines((prev) => [...prev, {
              id: `ask-skip-${Date.now()}`,
              kind: 'tool_result',
              text: '● User skipped questions',
            }])
            if (stream) {
              stream.respondAskUser(JSON.stringify('Skipped'))
            }
            setState((prev) => ({ ...prev, askUserRequest: null }))
          }}
        />
      )}

      {/* Prompt input (Claude Code-style bordered box) */}
      <PromptInput
        model={state.model}
        isLoading={state.isLoading}
        isActive={!showHelp && resumeSessions === null && !showModelSelector && !state.askUserRequest}
        verbose={state.verbose}
        planning={planning}
        logMode={logMode !== null}
        queuedMessages={messageQueue.map(m => m.text)}
        history={historyManager}
        updateHint={updateHint}
        serverState={serverState}
        onSubmit={handleSubmit}
        onInterrupt={handleInterrupt}
        onToggleVerbose={handleToggleVerbose}
        onEmptyPaste={onEmptyPaste}
      />
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

