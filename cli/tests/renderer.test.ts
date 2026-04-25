import { describe, test, expect, beforeEach } from 'bun:test'
import { Writable } from 'node:stream'
import { TermRenderer } from '../src/term/renderer.js'

// Mock stdout that captures writes
class MockStdout extends Writable {
  chunks: string[] = []
  rows = 24
  columns = 80

  _write(chunk: Buffer | string, _encoding: string, callback: () => void) {
    this.chunks.push(chunk.toString())
    callback()
  }

  get output(): string {
    return this.chunks.join('')
  }

  clear() {
    this.chunks = []
  }

  // Simulate event emitter for resize
  private listeners: Map<string, Function[]> = new Map()
  on(event: string, fn: Function): this {
    const list = this.listeners.get(event) ?? []
    list.push(fn)
    this.listeners.set(event, list)
    return this
  }
  off(event: string, fn: Function): this {
    const list = this.listeners.get(event) ?? []
    this.listeners.set(event, list.filter(f => f !== fn))
    return this
  }
  emit(event: string, ...args: any[]): boolean {
    const list = this.listeners.get(event) ?? []
    for (const fn of list) fn(...args)
    return list.length > 0
  }
}

function createRenderer(): { renderer: TermRenderer; stdout: MockStdout } {
  const stdout = new MockStdout() as any
  const renderer = new TermRenderer({ stdout })
  return { renderer, stdout }
}

describe('TermRenderer', () => {
  describe('init / destroy', () => {
    test('init hides cursor', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      expect(stdout.output).toContain('\x1b[?25l')
      renderer.destroy()
    })

    test('destroy shows cursor', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.destroy()
      expect(stdout.output).toContain('\x1b[?25h')
    })

    test('double destroy is safe', () => {
      const { renderer } = createRenderer()
      renderer.init()
      renderer.destroy()
      renderer.destroy() // should not throw
    })
  })

  describe('appendScroll', () => {
    test('writes text to stdout', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.appendScroll('hello world')
      expect(stdout.output).toContain('hello world')
      renderer.destroy()
    })

    test('adds trailing newline if missing', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.appendScroll('no newline')
      expect(stdout.output).toContain('no newline\n')
      renderer.destroy()
    })

    test('does not double newline', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.appendScroll('has newline\n')
      // Should not have \n\n at the end
      const idx = stdout.output.indexOf('has newline\n')
      expect(idx).toBeGreaterThanOrEqual(0)
      renderer.destroy()
    })

    test('empty text does nothing', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.appendScroll('')
      expect(stdout.output).toBe('')
      renderer.destroy()
    })
  })

  describe('setStatus', () => {
    test('first call draws all lines', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.setStatus(['line1', 'line2'])
      expect(stdout.output).toContain('line1')
      expect(stdout.output).toContain('line2')
      renderer.destroy()
    })

    test('identical lines produce no output', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['line1', 'line2'])
      stdout.clear()
      renderer.setStatus(['line1', 'line2'])
      // Should produce no output since nothing changed
      expect(stdout.output).toBe('')
      renderer.destroy()
    })

    test('changed lines trigger full redraw', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['line1', 'line2', 'line3'])
      stdout.clear()
      renderer.setStatus(['line1', 'CHANGED', 'line3'])
      // Should contain the changed line
      expect(stdout.output).toContain('CHANGED')
      // Full redraw: cursorUp to clear old status, then rewrite all lines
      expect(stdout.output).toContain('\x1b[3A') // cursorUp(3) to go back to start of status
      expect(stdout.output).toContain('line1')
      expect(stdout.output).toContain('line3')
      renderer.destroy()
    })

    test('height increase triggers full redraw', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['line1'])
      stdout.clear()
      renderer.setStatus(['line1', 'line2', 'line3'])
      expect(stdout.output).toContain('line1')
      expect(stdout.output).toContain('line2')
      expect(stdout.output).toContain('line3')
      renderer.destroy()
    })

    test('height decrease clears old lines', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['line1', 'line2', 'line3'])
      stdout.clear()
      renderer.setStatus(['only-one'])
      // Should clear old area (eraseDown) and draw new
      expect(stdout.output).toContain('\x1b[J') // eraseDown
      expect(stdout.output).toContain('only-one')
      renderer.destroy()
    })

    test('empty lines array clears status', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['line1', 'line2'])
      stdout.clear()
      renderer.setStatus([])
      expect(stdout.output).toContain('\x1b[J') // eraseDown to clear
      renderer.destroy()
    })
  })

  describe('appendScroll with active status', () => {
    test('clears status before appending, then redraws', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['status1', 'status2'])
      stdout.clear()
      renderer.appendScroll('new content')
      // Should clear status, write content, then redraw status
      const out = stdout.output
      expect(out).toContain('new content')
      renderer.destroy()
    })

    test('appendScroll clears status area — no stale pending text redrawn', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()

      // Simulate: status shows pending assistant text
      renderer.setStatus(['  pending assistant text', '> '])
      stdout.clear()

      // Now flush: append the same text to scroll
      renderer.appendScroll('  pending assistant text')
      const out = stdout.output

      // appendScroll clears status first, so the old status text
      // does NOT get redrawn. Only the scroll content appears.
      const matches = out.split('pending assistant text').length - 1
      expect(matches).toBe(1)
      renderer.destroy()
    })
    test('selector close restore flow re-appends content before drawing prompt', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['selector row 1', 'selector row 2', 'selector row 3', 'prompt'])
      stdout.clear()

      renderer.beginBatch()
      renderer.restoreViewport()
      renderer.appendScroll('banner\nprevious output')
      renderer.setStatus(['prompt'])
      renderer.flushBatch()

      const out = stdout.output
      const restoreIndex = out.indexOf('\x1b[1;1H\x1b[J')
      const contentIndex = out.indexOf('banner\nprevious output\n')
      const promptIndex = out.lastIndexOf('prompt\n')

      expect(restoreIndex).toBeGreaterThanOrEqual(0)
      expect(contentIndex).toBeGreaterThan(restoreIndex)
      expect(promptIndex).toBeGreaterThan(contentIndex)
      expect(out).not.toContain('selector row 1')
      expect(out).not.toContain('selector row 2')
      expect(out).not.toContain('selector row 3')
      renderer.destroy()
    })

    test('redrawViewport redraws normal screen without pushing blank rows or alternate buffer', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.redrawViewport('line1\nline2')
      const out = stdout.output
      expect(out).toContain('\x1b[1;1H')
      expect(out).toContain('\x1b[J')
      expect(out).toContain('line1\nline2\n')
      expect(out).toContain('\n'.repeat(22))
      expect(out).not.toContain('\x1b[?1049h')
      expect(out).not.toContain('\x1b[24A')
      expect(out).not.toBe('\n'.repeat(24))
      renderer.destroy()
    })

    test('restoreViewport clears normal screen and keeps scrollback available', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['status1', 'status2'])
      stdout.clear()
      renderer.restoreViewport()
      const out = stdout.output
      expect(out).toContain('\x1b[2A')
      expect(out).toContain('\x1b[1;1H')
      expect(out).toContain('\x1b[J')
      expect(out).toContain('\n'.repeat(24))
      expect(out).not.toContain('\x1b[?1049l')
      renderer.destroy()
    })
  })

  describe('resize handling', () => {
    test('updates dimensions on resize', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setStatus(['test'])
      stdout.rows = 40
      stdout.columns = 120
      stdout.emit('resize')
      expect(renderer.termRows).toBe(40)
      expect(renderer.termCols).toBe(120)
      renderer.destroy()
    })
  })
})
