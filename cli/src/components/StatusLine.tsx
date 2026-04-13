/**
 * StatusLine component — bottom bar showing session info, token usage, etc.
 */

import React from 'react'
import { Text, Box } from 'ink'

interface StatusLineProps {
  sessionId: string | null
  model: string
  cwd: string
  messageCount: number
}

export function StatusLine({ sessionId, model, cwd, messageCount }: StatusLineProps) {
  const shortSession = sessionId ? sessionId.slice(0, 8) : '—'
  const shortCwd = collapsePath(cwd)

  return (
    <Box>
      <Text dimColor>
        {shortCwd} · {model} · session {shortSession} · {messageCount} msgs
      </Text>
    </Box>
  )
}

function collapsePath(p: string): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? ''
  if (home && p.startsWith(home)) {
    return '~' + p.slice(home.length)
  }
  return p
}
