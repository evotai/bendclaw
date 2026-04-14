/**
 * ModelSelector — interactive model picker with type-to-filter.
 * Ported from Rust selector.rs model selection.
 */

import React, { useState } from 'react'
import { Text, Box, useInput } from 'ink'

interface ModelSelectorProps {
  models: string[]
  currentModel: string
  onSelect: (model: string) => void
  onCancel: () => void
}

export function ModelSelector({ models, currentModel, onSelect, onCancel }: ModelSelectorProps) {
  const [selectedIndex, setSelectedIndex] = useState(() => {
    const idx = models.indexOf(currentModel)
    return idx >= 0 ? idx : 0
  })
  const [filter, setFilter] = useState('')

  const filtered = filter.length > 0
    ? models.filter(m => m.toLowerCase().includes(filter.toLowerCase()))
    : models

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
      const model = filtered[selectedIndex]
      if (model) onSelect(model)
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
    if (ch && !key.ctrl && !key.meta) {
      setFilter((prev) => prev + ch)
      setSelectedIndex(0)
    }
  })

  return (
    <Box flexDirection="column">
      <Box marginBottom={1}>
        <Text bold>Select a model</Text>
        <Text dimColor>  (↑↓ navigate, type to filter, Enter select, Esc cancel)</Text>
      </Box>

      {filter.length > 0 && (
        <Box marginBottom={1}>
          <Text dimColor>filter: </Text>
          <Text color="cyan">{filter}</Text>
          <Text inverse>{' '}</Text>
          <Text dimColor> ({filtered.length} match{filtered.length !== 1 ? 'es' : ''})</Text>
        </Box>
      )}

      {filtered.length === 0 ? (
        <Text dimColor>  No matching models</Text>
      ) : (
        filtered.map((model, i) => {
          const isSelected = i === selectedIndex
          const isCurrent = model === currentModel
          const provider = providerFor(model)

          return (
            <Box key={model}>
              <Text color={isSelected ? 'cyan' : undefined} bold={isSelected}>
                {isSelected ? '❯ ' : '  '}
              </Text>
              <Text color={isSelected ? 'cyan' : undefined} bold={isSelected}>
                {model}
              </Text>
              {provider && <Text dimColor> ({provider})</Text>}
              {isCurrent && <Text color="green"> ✓</Text>}
            </Box>
          )
        })
      )}
    </Box>
  )
}

function providerFor(model: string): string | null {
  if (model.startsWith('claude-') || model.startsWith('anthropic/')) return 'anthropic'
  if (model.startsWith('gpt-') || model.startsWith('o1-') || model.startsWith('o3-') || model === 'o1' || model === 'o3') return 'openai'
  if (model.startsWith('gemini-')) return 'google'
  return null
}
