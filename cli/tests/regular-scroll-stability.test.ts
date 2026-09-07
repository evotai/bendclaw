import { expect, test } from 'bun:test'
import { buildToolCard } from '../src/render/output.js'
import { TermRenderer, type RendererTraceEntry } from '../src/term/renderer.js'
import { CURSOR_MARKER } from '../src/term/render-frame.js'
import { buildOutputBlocks, blocksToLines } from '../src/term/viewmodel/index.js'
import type { UIToolCall } from '../src/term/app/types.js'
import { ScreenHarness } from './helpers/screen.js'

async function paint(renderer: TermRenderer, screen: ScreenHarness): Promise<void> {
  renderer.requestRender()
  await Bun.sleep(25)
  await screen.settle()
}

function rows(call: UIToolCall, expanded = true): string[] {
  return blocksToLines(buildOutputBlocks(buildToolCard(call, expanded), { columns: 100 }))
}

test('ordinary terminal: expanded write append, ready, running, diff and completion preserve native reading position', async () => {
  const screen = new ScreenHarness(100, 24)
  const traces: RendererTraceEntry[] = []
  const renderer = new TermRenderer({ stdout: screen.stdout, trace: entry => traces.push(entry) })
  const content = Array.from({ length: 80 }, (_, i) => `const value${i} = ${i};`).join('\n') + '\n'
  let call: UIToolCall = { id: 'scroll-write', name: 'write', status: 'queued', argsComplete: false, args: { path: 'src/live.ts', content } }
  let expanded = false
  let committed: string[] | null = null
  renderer.init()
  renderer.setRenderCallback(() => ({
    lines: [...Array.from({ length: 50 }, (_, i) => `history ${i}`), ...(committed ?? rows(call, expanded)), 'spinner', `> ${CURSOR_MARKER}`, 'footer'],
    bottomAnchor: true,
  }))
  try {
    await paint(renderer, screen)
    expanded = true
    await paint(renderer, screen)
    // Ctrl+O itself can legitimately relayout history. What must not happen
    // afterward is a new clear/replay for every generated line or tool status.
    traces.length = 0
    screen.terminal.scrollLines(-35)
    const readingTop = screen.terminal.buffer.active.viewportY
    const readingRows = screen.viewport()
    for (let i = 80; i < 84; i++) {
      call = { ...call, args: { ...call.args, content: `${call.args.content}const value${i} = ${i};\n` } }
      await paint(renderer, screen)
      expect(screen.terminal.buffer.active.viewportY).toBe(readingTop)
      expect(screen.viewport()).toEqual(readingRows)
    }
    for (const change of [
      { argsComplete: true },
      { status: 'running' as const },
      { details: { diff: '@@ -0,0 +1 @@\n+authoritative preview', preview: true } },
      { status: 'done' as const, result: 'Wrote content', durationMs: 12 },
    ]) {
      call = { ...call, ...change }
      await paint(renderer, screen)
      expect(screen.terminal.buffer.active.viewportY).toBe(readingTop)
      expect(screen.viewport()).toEqual(readingRows)
    }
    // Flushing the partial through the same history pipeline changes no rows.
    committed = rows(call)
    await paint(renderer, screen)
    expect(screen.viewport()).toEqual(readingRows)
    expect(traces.every(e => e.branch === 'differential_update' || e.branch === 'no_change')).toBe(true)
    const ansi = traces.flatMap(e => e.ansiWrites).join('')
    expect(ansi).not.toContain('\x1b[3J')
    expect(ansi).not.toContain('\x1b[?1049h')
    expect(screen.terminal.buffer.active.type).toBe('normal')
    const buffer = screen.terminal.buffer.active
    const all = Array.from({ length: buffer.length }, (_, i) => buffer.getLine(i)?.translateToString(true) ?? '')
    expect(all.filter(line => line === 'history 0')).toHaveLength(1)
  } finally {
    renderer.destroy()
  }
})

test('write prefix is deterministic across completion, failure, alias and cache reconstruction', () => {
  for (const name of ['write', 'file_write']) {
    let call: UIToolCall = { id: `stable-${name}`, name, status: 'queued', argsComplete: false,
      args: { path: 'a.ts', content: '/* comment\nconst first = 1;\n' } }
    const prefix = rows(call).slice(0, -4) // omit mutable tail and closing padding
    call = { ...call, args: { ...call.args, content: `${call.args.content}*/\nconst second = 2;\n` } }
    expect(rows(call).slice(0, prefix.length)).toEqual(prefix)
    for (const status of ['running', 'done', 'error'] as const) {
      const completed = { ...call, status, argsComplete: true, result: status === 'error' ? 'Permission denied' : 'Wrote content', details: { diff: '@@ -1 +1 @@\n-old\n+new' } }
      const rendered = rows(completed)
      expect(rendered.slice(0, prefix.length)).toEqual(prefix)
      // A different id forces a fresh cache: same bytes after reload/eviction.
      expect(rows({ ...completed, id: `fresh-${name}-${status}` })).toEqual(rendered)
      expect(buildToolCard(completed).some(line => line.diffText)).toBe(false)
      if (status === 'error') expect(rendered.join('\n')).toContain('Permission denied')
    }
  }
})

test('write progress stays below the stable body and result metadata is not lost', () => {
  const base: UIToolCall = { id: 'write-progress-tail', name: 'write', status: 'running', args: { path: 'a.txt', content: 'one\ntwo' } }
  const running = buildToolCard({ ...base, progress: 'Flushing file', details: { diff: '@@ -1 +1 @@\n-old\n+new' } })
  expect(running.map(line => line.text).join('\n')).toContain('Flushing file')
  expect(running.findIndex(line => line.text.includes('Flushing file'))).toBeGreaterThan(running.findIndex(line => line.toolCodePreview))
  const done = buildToolCard({ ...base, status: 'done', durationMs: 12, result: 'Saved', details: { created: true, bytes: 7 } })
  expect(done.map(line => line.text).join('\n')).toContain('created 7 B · 12ms')
  expect(done.map(line => line.text).join('\n')).toContain('Saved')
  expect(buildToolCard({ ...base, progress: '__evot_spill_event__ secret' }).some(line => line.text.includes('__evot_spill_event__'))).toBe(false)
})

test('write without retained content still shows its authoritative diff', () => {
  const call: UIToolCall = { id: 'legacy-write', name: 'write', status: 'done', args: { path: 'a.ts' }, details: { diff: '@@ -1 +1 @@\n-old\n+new' } }
  expect(buildToolCard(call).some(line => line.diffText?.includes('+new'))).toBe(true)
})

test('visible shrink preserves actual scrollback and does not scroll during clearing', async () => {
  const screen = new ScreenHarness(100, 24)
  const traces: RendererTraceEntry[] = []
  const renderer = new TermRenderer({ stdout: screen.stdout, trace: e => traces.push(e) })
  let extra = Array.from({ length: 8 }, (_, i) => `thinking ${i}`)
  renderer.init()
  renderer.setRenderCallback(() => ({
    lines: [...Array.from({ length: 50 }, (_, i) => `history ${i}`), ...extra, `> ${CURSOR_MARKER}`, 'footer'], bottomAnchor: true,
  }))
  try {
    await paint(renderer, screen)
    const oldBase = screen.terminal.buffer.active.baseY
    screen.terminal.scrollLines(-15)
    const reading = screen.viewport()
    const top = screen.terminal.buffer.active.viewportY
    traces.length = 0
    extra = []
    await paint(renderer, screen)
    expect(traces.at(-1)?.branch).toBe('differential_update')
    expect(traces.flatMap(e => e.ansiWrites).join('')).not.toContain('\x1b[3J')
    expect(screen.terminal.buffer.active.baseY).toBe(oldBase)
    expect(screen.terminal.buffer.active.viewportY).toBe(top)
    expect(screen.viewport()).toEqual(reading)
    screen.terminal.scrollToBottom()
    expect(screen.rowOf('footer')).toBe(15)
    expect(screen.viewport().slice(16).every(line => line === '')).toBe(true)
    expect(screen.terminal.buffer.active.cursorY).toBe(14)
    // The next append should use free rows rather than snapping to the bottom.
    extra = ['new output']
    await paint(renderer, screen)
    expect(screen.rowOf('footer')).toBe(16)
    expect(screen.terminal.buffer.active.baseY).toBe(oldBase)
  } finally {
    renderer.destroy()
  }
})
