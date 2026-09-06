import { expect, test } from 'bun:test'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import { buildToolCard } from '../src/render/output.js'
import { buildOutputBlocks } from '../src/term/viewmodel/output.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'

test('background commands use the full result command and dim header styling', () => {
  const command = 'bun test ' + 'some-long-test.ts '.repeat(30)
  const card = (expanded = false) => buildToolCard({ id: 't', name: 'task_output', args: { task_id: 'id' }, status: 'done', previewCommand: 'bun t…', details: { command, status: 'completed' } }, expanded)
  const blocks = buildOutputBlocks(card(), { columns: 100 })
  const text = blocksToLines(blocks).map(stripAnsi)
  expect(text.join('\n')).toContain('some-long-test.ts')
  expect(text.every(row => stringWidth(row) <= 100)).toBe(true)
  const commandRows = text.filter(row => row.includes('some-long-test'))
  expect(commandRows).toHaveLength(2)
  expect(commandRows.at(-1)?.trimEnd()).toEndWith('…')
  const spans = blocks.flatMap(block => block.lines.flatMap(line => line.spans))
  expect(spans.some(span => span.text.includes('bun test') && span.dim)).toBe(true)
  expect(blocksToLines(buildOutputBlocks(card(true), { columns: 100 })).length).toBeGreaterThan(text.length)
})

test('compact task cards hide output and count only output, not the protocol envelope', () => {
  const result = 'Task ID: id\nStatus: completed\nOutput file: /tmp/out\nExit code: 0\nOutput:\nfirst\nsecond\n'
  const call = { id: 't', name: 'task_output', args: { task_id: 'id' }, status: 'done' as const, result }
  const compact = buildToolCard(call)
  expect(compact.map(row => row.text).join('\n')).not.toContain('first')
  expect(compact.map(row => row.text).join('\n')).not.toContain('second')
  expect(compact.map(row => row.text).join('\n')).not.toContain('Output file:')
  expect(compact.map(row => row.text).join('\n')).toContain('+2 lines')
  expect(buildToolCard(call, true).map(row => row.text).join('\n')).toContain('Output file:')
})

test('bash hides all successful output, including a single line, until expanded', () => {
  for (const result of ['one', 'one\ntwo\nthree\nfour\nfive\n']) {
    const call = { id: 'b', name: 'bash', args: { command: 'test' }, status: 'done' as const, result }
    const compact = buildToolCard(call).map(row => stripAnsi(row.text)).join('\n')
    expect(compact).not.toContain('one')
    expect(compact).not.toContain('five')
    expect(compact).toContain(result === 'one' ? '+1 lines' : '+5 lines')
    expect(buildToolCard(call, true).map(row => row.text).join('\n')).toContain('one')
  }
})

test('failed background tasks still expose the error without expanding', () => {
  const card = buildToolCard({ id: 't', name: 'task_output', args: { task_id: 'id' }, status: 'done', details: { status: 'failed', exit_code: 1 }, result: 'Task ID: id\nStatus: failed\nOutput:\ncompilation failed' })
  expect(card.some(row => row.kind === 'error' && row.text.includes('compilation failed'))).toBe(true)
})

test('empty task output adds no body or folding hint', () => {
  const card = buildToolCard({ id: 't', name: 'task_output', args: { task_id: 'id' }, status: 'done', result: 'Task ID: id\nStatus: completed\nOutput file: /tmp/out\nExit code: 0' })
  expect(card).toHaveLength(2)
  expect(card.map(row => row.text).join('\n')).not.toContain('ctrl+o')
})
