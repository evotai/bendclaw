/**
 * ResultPicker — generic selection list for tool output results.
 *
 * Use case: after a tool returns a list of items (search results, files, etc.),
 * let the user pick one interactively. Supports type-to-filter, scrolling
 * viewport, and optional detail preview.
 *
 * Usage:
 *   <ResultPicker
 *     title="Search results"
 *     items={[{ label: 'foo.ts', detail: 'src/foo.ts', value: 'src/foo.ts' }]}
 *     onSelect={(item) => { ... }}
 *     onCancel={() => { ... }}
 *   />
 */

import React, { useState } from 'react'
import { Text, Box, useInput, useStdout } from 'ink'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface PickerItem<T = string> {
  /** Primary display text */
  label: string
  /** Secondary detail shown to the right (dimmed) */
  detail?: string
  /** Optional value passed back on selection — defaults to label */
  value?: T
}

interface ResultPickerProps<T = string> {
  title?: string
  items: PickerItem<T>[]
  /** Max visible rows before scrolling (default: terminal height - 6, min 5) */
  maxVisible?: number
  onSelect: (item: PickerItem<T>, index: number) => void
  onCancel: () => void
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function ResultPicker<T = string>({
  title,
  items,
  maxVisible,
  onSelect,
  onCancel,
}: ResultPickerProps<T>) {
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [filter, setFilter] = useState('')
  const { stdout } = useStdout()
  const termRows = stdout?.rows ?? 24
  const viewportSize = maxVisible ?? Math.max(5, termRows - 6)

  const filtered = filter.length > 0
    ? items.filter((item) => {
        const q = filter.toLowerCase()
        return item.label.toLowerCase().includes(q)
          || (item.detail ?? '').toLowerCase().includes(q)
      })
    : items

  // Clamp selection after filter changes
  const clampedIndex = Math.min(selectedIndex, Math.max(0, filtered.length - 1))

  // Scrolling viewport
  const scrollStart = Math.max(
    0,
    Math.min(clampedIndex - Math.floor(viewportSize / 2), filtered.length - viewportSize),
  )
  const visible = filtered.slice(scrollStart, scrollStart + viewportSize)

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
      const item = filtered[clampedIndex]
      if (item) onSelect(item, clampedIndex)
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
      {/* Header */}
      <Box marginBottom={1}>
        <Text bold>{title ?? 'Select an item'}</Text>
        <Text dimColor>  (↑↓ navigate, type to filter, Enter select, Esc cancel)</Text>
      </Box>

      {/* Filter input */}
      {filter.length > 0 && (
        <Box marginBottom={1}>
          <Text dimColor>filter: </Text>
          <Text color="cyan">{filter}</Text>
          <Text inverse>{' '}</Text>
          <Text dimColor> ({filtered.length}/{items.length})</Text>
        </Box>
      )}

      {/* Items */}
      {filtered.length === 0 ? (
        <Text dimColor>  No matching items</Text>
      ) : (
        <>
          {/* Scroll-up indicator */}
          {scrollStart > 0 && (
            <Text dimColor>  ↑ {scrollStart} more</Text>
          )}

          {visible.map((item, vi) => {
            const realIndex = scrollStart + vi
            const isSelected = realIndex === clampedIndex
            return (
              <Box key={realIndex}>
                <Text color={isSelected ? 'cyan' : undefined} bold={isSelected}>
                  {isSelected ? '❯ ' : '  '}
                </Text>
                <Text color={isSelected ? 'cyan' : undefined} bold={isSelected}>
                  {item.label}
                </Text>
                {item.detail && (
                  <Text dimColor>  {item.detail}</Text>
                )}
              </Box>
            )
          })}

          {/* Scroll-down indicator */}
          {scrollStart + viewportSize < filtered.length && (
            <Text dimColor>  ↓ {filtered.length - scrollStart - viewportSize} more</Text>
          )}
        </>
      )}

      {/* Footer */}
      <Box marginTop={1}>
        <Text dimColor italic>
          {filtered.length > 0
            ? `${clampedIndex + 1}/${filtered.length}`
            : '0 items'}
        </Text>
      </Box>
    </Box>
  )
}
