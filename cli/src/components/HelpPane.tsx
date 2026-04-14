/**
 * HelpPane — bordered help overlay with commands and keyboard shortcuts.
 * Modeled after Claude Code's HelpV2.
 */

import React from 'react'
import { Text, Box, useInput, useStdout } from 'ink'
import { COMMANDS } from '../commands/index.js'
import { padRight } from '../utils/format.js'

interface HelpPaneProps {
  onDismiss: () => void
}

export function HelpPane({ onDismiss }: HelpPaneProps) {
  const { stdout } = useStdout()
  const columns = stdout?.columns ?? 120
  const borderLine = '─'.repeat(columns)

  useInput((_ch, key) => {
    if (key.escape) {
      onDismiss()
    }
  })

  return (
    <Box flexDirection="column">
      {/* Header */}
      <Text dimColor>{borderLine}</Text>
      <Box>
        <Text bold color="cyan">{' Help '}</Text>
        <Text dimColor> — press Escape to dismiss</Text>
      </Box>
      <Text dimColor>{borderLine}</Text>

      {/* Keyboard shortcuts */}
      <Box flexDirection="column" marginTop={1} marginLeft={2}>
        <Text bold underline>Keyboard Shortcuts</Text>
        <Text>{''}</Text>
        <Shortcut keys="Enter" desc="Submit message" />
        <Shortcut keys="Alt+Enter" desc="New line" />
        <Shortcut keys="Ctrl+C" desc="Clear input / exit" />
        <Shortcut keys="Ctrl+L" desc="Clear all input" />
        <Shortcut keys="Ctrl+O" desc="Toggle verbose output" />
        <Shortcut keys="Ctrl+U" desc="Clear line before cursor" />
        <Shortcut keys="Ctrl+K" desc="Clear line after cursor" />
        <Shortcut keys="Ctrl+W" desc="Delete word before cursor" />
        <Shortcut keys="Ctrl+A" desc="Move to start of line" />
        <Shortcut keys="Ctrl+E" desc="Move to end of line" />
        <Shortcut keys="Up/Down" desc="Navigate history" />
        <Shortcut keys="Tab" desc="Autocomplete commands/paths" />
        <Shortcut keys="Escape" desc="Interrupt / dismiss" />
      </Box>

      {/* Commands */}
      <Box flexDirection="column" marginTop={1} marginLeft={2}>
        <Text bold underline>Commands</Text>
        <Text>{''}</Text>
        {COMMANDS.map((cmd) => (
          <Box key={cmd.name}>
            <Text color="cyan">{padRight(cmd.usage ?? cmd.name, 24)}</Text>
            <Text dimColor>{cmd.description}</Text>
          </Box>
        ))}
      </Box>

      <Box marginTop={1} marginLeft={2}>
        <Text dimColor italic>Tip: commands can be abbreviated (e.g. /h for /help)</Text>
      </Box>

      <Text dimColor>{borderLine}</Text>
    </Box>
  )
}

function Shortcut({ keys, desc }: { keys: string; desc: string }) {
  return (
    <Box>
      <Text bold>{padRight(keys, 16)}</Text>
      <Text dimColor>{desc}</Text>
    </Box>
  )
}
