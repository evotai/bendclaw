/**
 * OutputView — renders OutputLines using Ink's <Static>.
 * Items are appended once and never re-rendered.
 */

import React from 'react'
import { Static, Text, Box } from 'ink'
import type { OutputLine } from '../utils/outputLines.js'

interface Props {
  banner: React.ReactNode
  lines: OutputLine[]
}

type StaticItem =
  | { kind: 'banner'; id: string; node: React.ReactNode }
  | { kind: 'line'; id: string; line: OutputLine }

export function OutputView({ banner, lines }: Props) {
  const items: StaticItem[] = [
    { kind: 'banner', id: '__banner__', node: banner },
    ...lines.map((line) => ({ kind: 'line' as const, id: line.id, line })),
  ]

  return (
    <Static items={items}>
      {(item) => {
        if (item.kind === 'banner') {
          return <React.Fragment key={item.id}>{item.node}</React.Fragment>
        }
        return <OutputLineView key={item.id} line={item.line} />
      }}
    </Static>
  )
}

function OutputLineView({ line }: { line: OutputLine }) {
  switch (line.kind) {
    case 'user':
      return (
        <Box marginTop={1}>
          <Text bold color="yellow">{'❯ '}</Text>
          <Text bold>{line.text}</Text>
        </Box>
      )
    case 'assistant':
      return (
        <Box marginTop={1}>
          <Text>{'  '}{line.text}</Text>
        </Box>
      )
    case 'tool':
      return <ToolLineView text={line.text} />
    case 'verbose':
      return <VerboseLineView text={line.text} />
    case 'error':
      return (
        <Box>
          <Text color="red">{line.text}</Text>
        </Box>
      )
    case 'system':
      return (
        <Box>
          <Text dimColor>{line.text}</Text>
        </Box>
      )
    case 'run_summary':
      return (
        <Box>
          <Text dimColor>{line.text}</Text>
        </Box>
      )
    default:
      return null
  }
}

function ToolLineView({ text }: { text: string }) {
  // Badge line: [tool_name] call / [tool_name] completed / [tool_name] failed
  const badgeMatch = text.match(/^\[([^\]]+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const isCompleted = rest.startsWith('completed')
    const isFailed = rest.startsWith('failed')
    const isCall = rest.startsWith('call')
    let color: string = 'yellow'
    if (isCompleted) color = 'green'
    if (isFailed) color = 'red'
    if (isCall) color = 'yellow'
    return (
      <Box marginTop={1}>
        <Text color={color} bold>[{badge}]</Text>
        {rest ? <Text dimColor> {rest}</Text> : null}
      </Box>
    )
  }
  // Detail line (preview command, args, diff)
  if (text.startsWith('  ')) {
    return (
      <Box>
        <Text dimColor>{text}</Text>
      </Box>
    )
  }
  return (
    <Box>
      <Text>{text}</Text>
    </Box>
  )
}

function VerboseLineView({ text }: { text: string }) {
  // Badge line: [LLM] ... or [COMPACT] ...
  const badgeMatch = text.match(/^\[(\w+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const isCompleted = rest.startsWith('completed') || rest.startsWith('·')
    let color: string = 'yellow'
    if (badge === 'COMPACT') color = 'green'
    if (isCompleted) color = rest.includes('error') ? 'red' : 'green'
    return (
      <Box marginTop={1}>
        <Text color={color} bold>[{badge}]</Text>
        {rest ? <Text dimColor> {rest}</Text> : null}
      </Box>
    )
  }
  // Detail line (indented with spaces)
  return (
    <Box>
      <Text dimColor>{text}</Text>
    </Box>
  )
}
