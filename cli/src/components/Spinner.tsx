/**
 * Spinner component — animated loading indicator with two phases:
 *   Thinking (LLM) / Executing (tool)
 * Shows "LLM slow…" or "Executing slow…" in red after 8s of no activity.
 */

import React, { useState, useEffect, useRef } from 'react'
import { Text, Box } from 'ink'
import { setTerminalTitle } from '../repl/server.js'

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
const SPINNER_INTERVAL = 100
const SLOW_THRESHOLD_MS = 8000
const SHOW_TOKENS_AFTER_MS = 30000

const TITLE_GLYPHS = ['·', '•', '·']

function humanDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  const secs = Math.floor(ms / 100) / 10
  if (secs < 60) return `${secs.toFixed(1)}s`
  const totalSecs = Math.floor(ms / 1000)
  const mins = Math.floor(totalSecs / 60)
  const rem = totalSecs % 60
  return rem > 0 ? `${mins}m${rem}s` : `${mins}m`
}

function formatTokens(count: number): string {
  if (count >= 1000) return `${(count / 1000).toFixed(1)}k`
  return `${count}`
}

interface SpinnerProps {
  toolName?: string
  tokenCount?: number
  lastTokenAt?: number
  /** True when markdown tokens are actively streaming — suppresses "slow" state */
  streaming?: boolean
}

interface SpinnerTick {
  frame: number
  elapsed: number
  glimmerPos: number
  slow: boolean
}

type Phase = 'thinking' | 'executing'

export function Spinner({ toolName, tokenCount = 0, lastTokenAt, streaming = false }: SpinnerProps) {
  const [tick, setTick] = useState<SpinnerTick>({ frame: 0, elapsed: 0, glimmerPos: -2, slow: false })
  const phaseStartRef = useRef(Date.now())
  const phaseRef = useRef<Phase>(toolName ? 'executing' : 'thinking')
  const lastTokenAtRef = useRef(lastTokenAt)
  lastTokenAtRef.current = lastTokenAt
  const streamingRef = useRef(streaming)
  streamingRef.current = streaming

  // Detect phase changes and reset timer
  const currentPhase: Phase = toolName ? 'executing' : 'thinking'
  if (currentPhase !== phaseRef.current) {
    phaseRef.current = currentPhase
    phaseStartRef.current = Date.now()
  }

  useEffect(() => {
    const timer = setInterval(() => {
      const now = Date.now()
      const sincePhaseStart = now - phaseStartRef.current
      // Thinking: slow only if no tokens received recently
      // Executing: slow only if tool running too long
      const lta = lastTokenAtRef.current
      const isThinking = phaseRef.current === 'thinking'
      const hasRecentTokens = isThinking && lta != null && (now - lta) < SLOW_THRESHOLD_MS
      const slow = sincePhaseStart > SLOW_THRESHOLD_MS && !hasRecentTokens && !streamingRef.current
      setTick((prev) => ({
        frame: (prev.frame + 1) % SPINNER_FRAMES.length,
        elapsed: sincePhaseStart,
        glimmerPos: prev.glimmerPos + 1 > 30 ? -2 : prev.glimmerPos + 1,
        slow,
      }))
    }, SPINNER_INTERVAL)
    return () => clearInterval(timer)
  }, [])

  // Terminal tab title animation
  const titleIdx = useRef(0)
  useEffect(() => {
    const timer = setInterval(() => {
      titleIdx.current = (titleIdx.current + 1) % TITLE_GLYPHS.length
      setTerminalTitle(TITLE_GLYPHS[titleIdx.current])
    }, 500)
    return () => {
      clearInterval(timer)
      setTerminalTitle()
    }
  }, [])

  const { frame, elapsed, glimmerPos, slow } = tick
  const showTokens = elapsed > SHOW_TOKENS_AFTER_MS && tokenCount > 0
  const isTool = currentPhase === 'executing'

  let label: string
  if (slow) {
    label = isTool ? 'Executing slow…' : 'LLM slow…'
  } else {
    label = isTool ? 'Executing…' : 'Thinking…'
  }

  let status = humanDuration(elapsed)
  if (showTokens) {
    status += ` · ${formatTokens(tokenCount)} tokens`
  }

  const color = slow ? 'red' : 'cyan'

  return (
    <Box flexDirection="column">
      <Box>
        <Text color={color}>{SPINNER_FRAMES[frame]} </Text>
        {slow
          ? <Text color="red">{label}</Text>
          : <GlimmerText text={label} pos={glimmerPos} />
        }
        <Text dimColor> ({status}) · esc to interrupt</Text>
      </Box>
    </Box>
  )
}

function GlimmerText({ text, pos }: { text: string; pos: number }) {
  const start = pos - 1
  const end = pos + 1

  return (
    <Text>
      {[...text].map((ch, i) => {
        if (i >= start && i <= end) {
          return <Text key={i} color="white" bold>{ch}</Text>
        }
        return <Text key={i} dimColor>{ch}</Text>
      })}
    </Text>
  )
}
