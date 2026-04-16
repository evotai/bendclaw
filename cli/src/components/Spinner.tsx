/**
 * Spinner component — animated loading indicator with phases, glimmer effect,
 * slow detection, token count, and terminal tab title.
 */

import React, { useState, useEffect, useRef } from 'react'
import { Text, Box } from 'ink'

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
}

interface SpinnerTick {
  frame: number
  elapsed: number
  glimmerPos: number
  slow: boolean
}

export function Spinner({ toolName, tokenCount = 0, lastTokenAt }: SpinnerProps) {
  const [tick, setTick] = useState<SpinnerTick>({ frame: 0, elapsed: 0, glimmerPos: -2, slow: false })
  const startRef = useRef(Date.now())
  const lastTokenAtRef = useRef(lastTokenAt)
  lastTokenAtRef.current = lastTokenAt

  // Reset timer when phase changes (thinking → executing or between tools)
  const prevToolRef = useRef(toolName)
  useEffect(() => {
    if (prevToolRef.current !== toolName) {
      prevToolRef.current = toolName
      startRef.current = Date.now()
      lastTokenAtRef.current = undefined
    }
  }, [toolName])

  useEffect(() => {
    const timer = setInterval(() => {
      const now = Date.now()
      const lta = lastTokenAtRef.current
      const sinceActivity = lta != null ? now - lta : now - startRef.current
      setTick((prev) => ({
        frame: (prev.frame + 1) % SPINNER_FRAMES.length,
        elapsed: now - startRef.current,
        glimmerPos: prev.glimmerPos + 1 > 30 ? -2 : prev.glimmerPos + 1,
        slow: sinceActivity > SLOW_THRESHOLD_MS,
      }))
    }, SPINNER_INTERVAL)
    return () => clearInterval(timer)
  }, [])

  // Terminal tab title animation
  const titleIdx = useRef(0)
  useEffect(() => {
    const timer = setInterval(() => {
      titleIdx.current = (titleIdx.current + 1) % TITLE_GLYPHS.length
      process.stdout.write(`\x1b]0;${TITLE_GLYPHS[titleIdx.current]} Evot\x07`)
    }, 500)
    return () => {
      clearInterval(timer)
      process.stdout.write('\x1b]0;Evot\x07')
    }
  }, [])

  const { frame, elapsed, glimmerPos, slow } = tick
  const showTokens = elapsed > SHOW_TOKENS_AFTER_MS && tokenCount > 0
  const isTool = !!toolName

  // Label: two states × two phases
  let label: string
  if (slow) {
    label = isTool ? 'Executing slow…' : 'LLM slow…'
  } else {
    label = isTool ? 'Executing…' : 'Thinking…'
  }

  // Status
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

/** Renders text with a sweeping bright-white highlight (glimmer effect). */
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
