/**
 * SessionSelector — interactive session picker with type-to-filter.
 * Ported from Rust selector.rs.
 */

import React, { useState } from 'react'
import { Text, Box, useInput } from 'ink'
import type { SessionMeta } from '../native/index.js'
import { relativeTime, padRight } from '../utils/format.js'

interface SessionSelectorProps {
  sessions: SessionMeta[]
  currentCwd?: string
  onSelect: (session: SessionMeta) => void
  onCancel: () => void
}

export function SessionSelector({ sessions, currentCwd, onSelect, onCancel }: SessionSelectorProps) {
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [filter, setFilter] = useState('')

  const filtered = filter.length > 0
    ? sessions.filter(s => {
        const q = filter.toLowerCase()
        return s.session_id.toLowerCase().includes(q)
          || (s.title ?? '').toLowerCase().includes(q)
      })
    : sessions

  useInput((ch, key) => {
    if (key.upArrow) {
      setSelectedIndex((prev) => Math.max(0, prev - 1))
      return
    }
    if (key.downArrow) {
      setSelectedIndex((prev) => Math.min(filtered.length - 1, prev + 1))
      return
    }
    if (key.return) {
      const session = filtered[selectedIndex]
      if (session) onSelect(session)
      return
    }
    if (key.escape || (key.ctrl && ch === 'c')) {
      onCancel()
      return
    }
    if (key.backspace || key.delete) {
      setFilter((prev) => prev.slice(0, -1))
      setSelectedIndex(0)
      return
    }
    // Type-to-filter
    if (ch && !key.ctrl && !key.meta) {
      setFilter((prev) => prev + ch)
      setSelectedIndex(0)
    }
  })

  return (
    <Box flexDirection="column">
      <Box marginBottom={1}>
        <Text bold>Resume a conversation</Text>
        <Text dimColor>  (↑↓ navigate, type to filter, Enter select, Esc cancel)</Text>
      </Box>

      {/* Filter input */}
      {filter.length > 0 && (
        <Box marginBottom={1}>
          <Text dimColor>filter: </Text>
          <Text color="cyan">{filter}</Text>
          <Text inverse>{' '}</Text>
          <Text dimColor> ({filtered.length} match{filtered.length !== 1 ? 'es' : ''})</Text>
        </Box>
      )}

      {filtered.length === 0 ? (
        <Text dimColor>  No matching sessions</Text>
      ) : (
        filtered.map((s, i) => {
          const isSelected = i === selectedIndex
          const id = s.session_id.slice(0, 8)
          const title = s.title || '(untitled)'
          const displayTitle = title.length > 50 ? title.slice(0, 49) + '…' : title
          const time = relativeTime(s.updated_at)

          const cwdMarker = currentCwd && s.cwd === currentCwd ? '*' : ' '

          return (
            <Box key={s.session_id}>
              <Text color={isSelected ? 'cyan' : undefined} bold={isSelected}>
                {isSelected ? '❯ ' : '  '}
              </Text>
              <Text dimColor>{cwdMarker}</Text>
              <Text color={isSelected ? 'cyan' : undefined} bold={isSelected}>
                {id}
              </Text>
              <Text color={isSelected ? 'white' : undefined}>
                {'  '}{padRight(displayTitle, 50)}
              </Text>
              <Text dimColor>{'  '}{time}</Text>
            </Box>
          )
        })
      )}
    </Box>
  )
}
