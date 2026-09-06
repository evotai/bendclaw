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
  expect(text.filter(row => row.includes('some-long-test'))).toHaveLength(1)
  expect(text.find(row => row.includes('some-long-test'))?.trimEnd()).toEndWith('…')
  const spans = blocks.flatMap(block => block.lines.flatMap(line => line.spans))
  expect(spans.some(span => span.text.includes('bun test') && span.dim)).toBe(true)
  expect(blocksToLines(buildOutputBlocks(card(true), { columns: 100 })).length).toBeGreaterThan(text.length)
})

test('compact task cards show actual output without the protocol envelope', () => {
  const result = 'Task ID: id\nStatus: completed\nOutput file: /tmp/out\nExit code: 0\nOutput:\nfirst\nsecond\n'
  const call = { id: 't', name: 'task_output', args: { task_id: 'id' }, status: 'done' as const, result }
  const compact = buildToolCard(call)
  expect(compact.filter(row => row.shellOutput).map(row => row.text)).toEqual(['  first', '  second'])
  expect(compact.map(row => row.text).join('\n')).not.toContain('Output file:')
  expect(compact.map(row => row.text).join('\n')).not.toContain('ctrl+o')
  expect(buildToolCard(call, true).map(row => row.text).join('\n')).toContain('Output file:')
})

test('bash previews last three lines and counts only hidden output', () => {
  const card = buildToolCard({ id: 'b', name: 'bash', args: { command: 'test' }, status: 'done', result: 'one\ntwo\nthree\nfour\nfive\n' })
  expect(card.filter(row => row.shellOutput).map(row => row.text)).toEqual(['  three', '  four', '  five'])
  expect(card.map(row => stripAnsi(row.text)).join('\n')).toContain('+2 lines')
})

test('empty task output adds no body or folding hint', () => {
  const card = buildToolCard({ id: 't', name: 'task_output', args: { task_id: 'id' }, status: 'done', result: 'Task ID: id\nStatus: completed\nOutput file: /tmp/out\nExit code: 0' })
  expect(card.filter(row => row.shellOutput)).toEqual([])
  expect(card.map(row => row.text).join('\n')).not.toContain('ctrl+o')
})
