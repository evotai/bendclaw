import { expect, test } from 'bun:test'
import { replayUserText } from '../src/session/task-notification.js'
import { transcriptToMessages } from '../src/session/transcript.js'
import { messagesToOutputLines } from '../src/render/output.js'
import { buildOutputBlocks } from '../src/term/viewmodel/output.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import stripAnsi from 'strip-ansi'

const historical = `<task-notification>
<task-id>38d5f299-ea7e-4c3d-b24f-aa704129d831</task-id>
<status>killed</status>
<exit-code>-1</exit-code>
<summary>Command "run tests" was cancelled by the user</summary>
<output-file>/tmp/tool-results/output.txt</output-file>
</task-notification>`

test('legacy task notifications replay as system notices, never user bubbles', () => {
  const messages = transcriptToMessages([{ type: 'user', text: historical }])
  expect(messages).toHaveLength(1)
  expect(messages[0]?.role).toBe('system')
  const lines = messagesToOutputLines(messages)
  expect(lines.every(line => line.kind === 'system')).toBe(true)
  const rendered = blocksToLines(buildOutputBlocks(lines, { columns: 100 })).map(stripAnsi).join('\n')
  expect(rendered).toContain('Background task cancelled · 38d5f299 · exit -1')
  expect(rendered).not.toContain('<task-notification>')
  expect(rendered).not.toContain('08:00')
  expect(rendered).not.toContain('┃')
})

test('mixed user text and multiple notifications preserve order and user content', () => {
  const text = `check this\n${historical}\nthen this\n${historical.replace('killed', 'completed')}\nkeep this too`
  const parts = replayUserText(text)
  expect(parts.map(part => part.kind)).toEqual(['user', 'task-notification', 'user', 'task-notification', 'user'])
  expect(parts.filter(part => part.kind === 'user').map(part => part.text)).toEqual(['check this', 'then this', 'keep this too'])
  expect(replayUserText(historical.replaceAll('\n', '\r\n'))[0]?.kind).toBe('task-notification')
})

test('quoted, incomplete, and unrecognized envelopes remain ordinary user text', () => {
  for (const text of [
    `explain <task-notification>`,
    historical.replace('</task-notification>', ''),
    historical.replace('killed', 'future-status'),
    historical.replace('<task-id>', '<unknown-id>'),
    `\`\`\`xml\n${historical}\n\`\`\``,
    `~~~xml\n${historical}\n~~~`,
  ]) expect(replayUserText(text)).toEqual([{ kind: 'user', text }])
})

test('unknown replay timestamps are absent but known live timestamps survive', () => {
  const restored = messagesToOutputLines(transcriptToMessages([{ type: 'user', text: 'hello' }]))
  expect(restored[0]).not.toHaveProperty('timestamp')
  expect(restored[0]?.kind).toBe('user')
  const live = messagesToOutputLines([{ id: 'live', role: 'user', text: 'hello', timestamp: 1700000000000 }])
  expect(live[0]?.timestamp).toBe(1700000000000)
})
