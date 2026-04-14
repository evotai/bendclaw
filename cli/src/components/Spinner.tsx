/**
 * Spinner component — animated loading indicator with phases, glimmer effect,
 * stalled detection, token count, and terminal tab title.
 * Ported from Rust spinner.rs.
 */

import React, { useState, useEffect, useRef } from 'react'
import { Text, Box } from 'ink'
import { shouldAnimateTerminalTitle } from '../utils/streaming.js'

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
const SPINNER_INTERVAL = 80
const STALLED_THRESHOLD_MS = 3000
const SHOW_TOKENS_AFTER_MS = 30000

const VERBS = [
  'Defragmenting', 'Denormalizing', 'Sharding', 'Vacuuming', 'Reindexing',
  'Compacting', 'Coalescing', 'Partitioning', 'Materializing', 'Checkpointing',
  'Tombstoning', 'Backfilling', 'Rehashing', 'Journaling', 'Snapshotting',
  'Gossipping', 'Quiescing', 'Fencing', 'Spilling', 'Compressing',
]

const TITLE_GLYPHS = ['·', '•', '·']

function pickVerb(): string {
  return VERBS[Math.floor(Math.random() * VERBS.length)]!
}

function humanDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  const secs = Math.floor(ms / 1000)
  if (secs < 60) return `${secs}s`
  const mins = Math.floor(secs / 60)
  const rem = secs % 60
  return rem > 0 ? `${mins}m${rem}s` : `${mins}m`
}

function formatTokens(count: number): string {
  if (count >= 1000) return `${(count / 1000).toFixed(1)}k`
  return `${count}`
}

interface SpinnerProps {
  toolName?: string
  progressText?: string
  tokenCount?: number
  lastTokenAt?: number
}

export function Spinner({ toolName, progressText, tokenCount = 0, lastTokenAt }: SpinnerProps) {
  const [frame, setFrame] = useState(0)
  const verbRef = useRef(pickVerb())
  const startRef = useRef(Date.now())
  const [elapsed, setElapsed] = useState(0)
  const [glimmerPos, setGlimmerPos] = useState(-2)

  useEffect(() => {
    const timer = setInterval(() => {
      setFrame((prev) => (prev + 1) % SPINNER_FRAMES.length)
      setElapsed(Date.now() - startRef.current)
      setGlimmerPos((prev) => {
        const next = prev + 1
        return next > 30 ? -2 : next  // reset glimmer sweep
      })
    }, SPINNER_INTERVAL)
    return () => clearInterval(timer)
  }, [])

  // Terminal tab title animation is opt-in because direct stdout writes can
  // fight Ink's renderer and cause visible screen redraws in some terminals.
  const titleIdx = useRef(0)
  useEffect(() => {
    if (!shouldAnimateTerminalTitle()) {
      return
    }
    const timer = setInterval(() => {
      titleIdx.current = (titleIdx.current + 1) % TITLE_GLYPHS.length
      process.stdout.write(`\x1b]0;${TITLE_GLYPHS[titleIdx.current]} evot\x07`)
    }, 500)
    return () => {
      clearInterval(timer)
      process.stdout.write('\x1b]0;evot\x07')
    }
  }, [])

  const stalled = lastTokenAt != null && (Date.now() - lastTokenAt) > STALLED_THRESHOLD_MS
  const showTokens = elapsed > SHOW_TOKENS_AFTER_MS && tokenCount > 0

  // Build label
  let label: string
  if (toolName) {
    label = `Running ${toolName}…`
  } else {
    label = `${verbRef.current}…`
  }

  // Build status
  let status = humanDuration(elapsed)
  if (showTokens) {
    status += ` · ${formatTokens(tokenCount)} tokens`
  }

  // Glimmer effect: sweep bright chars across the label
  const glimmerCurrent = glimmerPos

  // Progress lines (tool output preview)
  const progressLines = progressText
    ? progressText.split('\n').slice(-5).map(l => l.slice(0, 120))
    : []

  return (
    <Box flexDirection="column">
      {/* Progress lines above spinner */}
      {progressLines.length > 0 && progressLines.map((line, i) => (
        <Box key={i}>
          <Text dimColor>  {line}</Text>
        </Box>
      ))}

      {/* Spinner line */}
      <Box>
        <Text color={stalled ? 'red' : 'cyan'}>{SPINNER_FRAMES[frame]} </Text>
        <GlimmerText text={label} pos={glimmerCurrent} stalled={stalled} />
        <Text dimColor> ({status}) · esc to interrupt</Text>
      </Box>
    </Box>
  )
}

/** Renders text with a sweeping bright-white highlight (glimmer effect). */
function GlimmerText({ text, pos, stalled }: { text: string; pos: number; stalled: boolean }) {
  if (stalled) {
    return <Text color="red">{text}</Text>
  }

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
