/**
 * VerboseEventLine — colored badges for [COMPACT] and [LLM] events.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { VerboseEvent } from '../state/AppState.js'

interface Props {
  event: VerboseEvent
}

export function VerboseEventLine({ event }: Props) {
  const lines = event.text.split('\n')
  const isCompact = event.kind === 'compact_call' || event.kind === 'compact_done'
  const isLlm = event.kind === 'llm_call' || event.kind === 'llm_completed'

  const [firstLine, ...rest] = lines

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
