import { getTheme } from '../src/render/theme/index.js'
import { expect, test } from 'bun:test'
import { createPatch } from 'diff'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import { buildDiffLines } from '../src/term/viewmodel/diff.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { buildOutputBlocks } from '../src/term/viewmodel/output.js'
import { buildToolCard } from '../src/render/output.js'

const render = (old: string, next: string, width: number) => blocksToLines([{ lines: buildDiffLines(createPatch('test.rs', old, next), width) }]).map(stripAnsi)

test('wide diff pairs old and new lines; narrow diff stays unified', () => {
  const wide = render('let a = 1;\n', 'let a = 2;\n', 140)
  const change = wide.find(text => text.includes('let a = 1;'))!
  expect(change).toContain('│')
  expect(change).toContain('let a = 2;')
  expect(wide.join('\n')).not.toContain('Before')
  expect(wide.join('\n')).not.toContain('After')
  expect(render('old\n', 'new\n', 80).join('\n')).not.toContain('│')
})

test('split handles unequal changes and independent line numbers', () => {
  const lines = render('a\nb\nc\n', 'a\nx\ny\nc\n', 140)
  const context = lines.find(text => /3\s+c/.test(text))
  expect(context).toMatch(/4\s+c/)
  expect(lines.filter(text => text.includes('│')).length).toBeGreaterThan(3)
})

test('long Unicode lines wrap within their own pane without losing content', () => {
  const lines = render('old\n', '中文🙂'.repeat(90) + '\n', 140)
  expect(lines.every(text => stringWidth(text) <= 140)).toBe(true)
  expect(lines.join('').match(/🙂/g)).toHaveLength(90)
})

test('edit gutters omit rails and signs while retaining diff fills and code', () => {
  const patch = createPatch('a', 'old - value\n', 'new + value ' + 'word '.repeat(60) + '\n')
  for (const width of [80, 140]) {
    const lines = buildDiffLines(patch, width)
    const fills = lines.flatMap(row => [row.bg, ...row.spans.map(span => span.bg)])
    expect(fills).toContain(getTheme().diffRemovedBg)
    expect(fills).toContain(getTheme().diffAddedBg)
    const plain = blocksToLines([{ lines }]).map(stripAnsi)
    expect(plain.join('\n')).not.toMatch(/\d+\s+[+-]\s/)
    expect(plain.join('\n')).toContain('old - value')
    expect(plain.join('\n')).toContain('new + value')
    expect(plain.every(row => stringWidth(row) <= width)).toBe(true)
    expect(plain.join('\n')).not.toContain('▎')
  }
})

test('live and settled cards share responsive layout', () => {
  const patch = createPatch('a', 'before\n', 'after\n')
  for (const status of ['running', 'done'] as const) {
    const lines = buildToolCard({ id: 'edit', name: 'edit', args: { path: 'a' }, status, details: { diff: patch }, result: 'updated' })
    expect(blocksToLines(buildOutputBlocks(lines, { columns: 150 })).map(stripAnsi).join('\n')).toContain('│')
    expect(blocksToLines(buildOutputBlocks(lines, { columns: 80 })).map(stripAnsi).join('\n')).not.toContain('│')
  }
})
