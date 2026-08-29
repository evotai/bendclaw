import { describe, test, expect, beforeEach } from 'bun:test'
import { Writable } from 'node:stream'
import stripAnsi from 'strip-ansi'
import { TermRenderer, CURSOR_MARKER } from '../src/term/renderer.js'
import { renderMarkdown } from '../src/render/markdown.js'
import { withColumns } from './helpers/stdout-columns.js'

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

// Helper: trigger a synchronous render by calling requestRender + flushing nextTick
async function renderFrame(renderer: TermRenderer): Promise<void> {
  renderer.requestRender()
  await new Promise(resolve => process.nextTick(resolve))
  // Wait for the scheduled render (MIN_RENDER_INTERVAL_MS = 16ms)
  await Bun.sleep(20)
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

  describe('differential rendering', () => {
    test('first render outputs all lines', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2', 'line3'])
      stdout.clear()
      await renderFrame(renderer)
      expect(stdout.output).toContain('line1')
      expect(stdout.output).toContain('line2')
      expect(stdout.output).toContain('line3')
      renderer.destroy()
    })

    test('identical frames only refresh hardware cursor visibility', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2'])
      await renderFrame(renderer)
      stdout.clear()
      await renderFrame(renderer)
      expect(stdout.output).toBe('\x1b[?25l')
      renderer.destroy()
    })

    test('invalidated rows repaint without clearing viewport or scrollback', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['history', `❯ hello${CURSOR_MARKER}`])
      await renderFrame(renderer)

      stdout.clear()
      renderer.invalidateRowsFrom(1)
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain('❯ hello')
      expect(out).toContain('\x1b[2K')
      expect(out).not.toContain('\x1b[2J')
      expect(out).not.toContain('\x1b[3J')
      expect(out).not.toContain('history')

      stdout.clear()
      await renderFrame(renderer)
      expect(stdout.output).not.toContain('❯ hello')
      renderer.destroy()
    })

    test('invalidating repaints every row of the live region, not just the caret row', async () => {
      // A mouse drag highlights a range of rows. Rewriting the cells is the only
      // way to release it, so one row is not enough.
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => [
        'committed transcript',
        '╭─────────╮',
        `│ draft${CURSOR_MARKER} │`,
        '╰─────────╯',
        'footer',
      ])
      await renderFrame(renderer)

      stdout.clear()
      renderer.invalidateRowsFrom(1)
      await renderFrame(renderer)

      const out = stdout.output
      for (const row of ['╭─────────╮', '│ draft', '╰─────────╯', 'footer']) {
        expect(out).toContain(row)
      }
      // Committed transcript above the live region is left alone: repainting
      // scrollback on every keystroke costs more than a stale highlight there.
      expect(out).not.toContain('committed transcript')
      renderer.destroy()
    })

    test('invalidating clamps a start row past the end of the frame', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['only line'])
      await renderFrame(renderer)

      stdout.clear()
      renderer.invalidateRowsFrom(99)
      await renderFrame(renderer)
      expect(stdout.output).toContain('only line')
      renderer.destroy()
    })

    test('bottom anchor leaves a short frame at its natural position', async () => {
      const { renderer } = createRenderer()
      ;(renderer as any).stdout.rows = 8
      renderer.init()
      renderer.setRenderCallback(() => ({
        lines: ['history 1', 'Interrupted.', 'ad', `❯ draft${CURSOR_MARKER}`, 'footer'],
        bottomAnchor: true,
      }))

      await renderFrame(renderer)

      const reset = '\x1b[0m\x1b]8;;\x07'
      // Content decides placement. Padding a short frame to the viewport would
      // pin the composer to the bottom of an almost-empty session.
      expect((renderer as any).previousLines).toEqual([
        `history 1${reset}`,
        `Interrupted.${reset}`,
        `ad${reset}`,
        `❯ draft${reset}`,
        `footer${reset}`,
      ])
      expect((renderer as any).hardwareCursorRow).toBe(3)
      renderer.destroy()
    })

    test('bottom anchor lets a short frame shrink in place without a full clear', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 8
      renderer.init()
      let thinking = ['thinking 0', 'thinking 1']
      renderer.setRenderCallback(() => ({
        lines: ['Interrupted.', ...thinking, `❯ ${CURSOR_MARKER}`, 'footer'],
        bottomAnchor: true,
      }))
      await renderFrame(renderer)

      stdout.clear()
      thinking = []
      await renderFrame(renderer)

      const lines = (renderer as any).previousLines as string[]
      // The frame follows its content up; no viewport re-anchor is involved
      // because the trailing edge was never on the bottom row.
      expect(lines).toHaveLength(3)
      expect(stripAnsi(lines.at(-2) ?? '')).toBe('❯ ')
      expect(stripAnsi(lines.at(-1) ?? '')).toBe('footer')
      expect(stdout.output).not.toContain('\x1b[2J')
      renderer.destroy()
    })

    test('appended lines use append fast path', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let lines = ['line1', 'line2']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)
      stdout.clear()
      lines = ['line1', 'line2', 'line3']
      await renderFrame(renderer)
      const out = stdout.output
      expect(out).toContain('line3')
      // Should not redraw line1 or line2
      expect(out).not.toContain('line1')
      expect(out).not.toContain('line2')
      renderer.destroy()
    })

    test('streaming table reflows visible rows through differential updates', async () => {
      const restore = withColumns(80)
      const { renderer, stdout } = createRenderer()
      stdout.rows = 16
      stdout.columns = 80
      renderer.init()
      let markdown = [
        '| Name | Value |',
        '| --- | --- |',
        '| first | short |',
      ].join('\n')
      renderer.setRenderCallback(() => [
        ...stripAnsi(renderMarkdown(markdown, { streaming: true })).split('\n'),
        'prompt',
      ])

      try {
        await renderFrame(renderer)
        stdout.clear()
        markdown += '\n| second | a much wider value that changes the live column geometry |'
        await renderFrame(renderer)

        const out = stdout.output
        expect(out).not.toContain('\x1b[2J\x1b[H')
        // The wider cell changes existing column geometry, so the header and
        // prior rows are repainted together with the newly arrived row.
        expect(out).toContain('Name')
        expect(out).toContain('second')
        expect(out).toContain('prompt')
      } finally {
        renderer.destroy()
        restore()
      }
    })

    test('changed middle line only redraws that line', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let lines = ['line1', 'line2', 'line3']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)
      stdout.clear()
      lines = ['line1', 'CHANGED', 'line3']
      await renderFrame(renderer)
      const out = stdout.output
      expect(out).toContain('CHANGED')
      expect(out).not.toContain('line1')
      expect(out).not.toContain('line3')
      renderer.destroy()
    })

    test('shrinking content clears extra lines', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let lines = ['line1', 'line2', 'line3']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)
      stdout.clear()
      lines = ['line1']
      await renderFrame(renderer)
      const out = stdout.output
      // Should contain clear line sequences for removed lines
      expect(out).toContain('\x1b[2K')
      renderer.destroy()
    })

    test('shrinking a visible frame clears removed rows without repainting the viewport', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 8
      renderer.init()

      let lines = Array.from({ length: 8 }, (_, i) => `old ${i}`)
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      stdout.clear()
      lines = ['history', 'Thinking...', '────────', '❯ ', '────────', 'footer']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain('\x1b[2J\x1b[H')
      expect(out).toContain('\x1b[2K')
      expect(out).toContain('history')
      expect(out).toContain('footer')
      renderer.destroy()
    })

    test('shrinking a scrolled frame preserves physical scrollback', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 8
      renderer.init()

      const history = Array.from({ length: 14 }, (_, i) => `history ${i}`)
      let lines = [
        ...history,
        'Thinking...',
        '────────',
        '❯ ',
        '────────',
        'footer',
        '',
      ]
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      stdout.clear()
      // Mirrors run completion: the stable history prefix is unchanged, the
      // spinner disappears, and prompt + footer shift upward by one row.
      // Clearing and re-homing here copies the old prompt into Warp scrollback.
      lines = [
        ...history,
        '────────',
        '❯ ',
        '────────',
        'footer',
        '',
      ]
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain('\x1b[2J')
      expect(out).not.toContain('\x1b[H')
      expect(out).not.toContain('\x1b[3J')
      expect(out).toContain('footer')
      renderer.destroy()
    })

    test('uses synchronized output wrapping', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['hello'])
      stdout.clear()
      await renderFrame(renderer)
      const out = stdout.output
      expect(out).toContain('\x1b[?2026h') // sync start
      expect(out).toContain('\x1b[?2026l') // sync end
      renderer.destroy()
    })
  })

  describe('off-viewport change (streaming markdown reflow)', () => {
    // When streaming, markdown re-renders the whole accumulated text each frame
    // and reflows earlier lines (table realign, list renumber). When a reflowed
    // line has scrolled above the visible viewport, no escape sequence can
    // address it, so the renderer falls back to a full redraw (matching pi's
    // renderer). A prior attempt to repaint in place instead desynced the
    // on-screen window from the terminal's real scrollback and made text
    // selections jump on scroll.
    const CLEAR_SCREEN = '\x1b[2J\x1b[H\x1b[3J'

    test('changing a line above the viewport triggers a full redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      const history = Array.from({ length: 30 }, (_, i) => `hist ${i}`)
      let lines = [...history, 's0', 's1', 's2', 's3']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      // Append more streamed lines so the viewport scrolls well past history.
      lines = [...history, 's0', 's1', 's2', 's3', 's4', 's5']
      await renderFrame(renderer)

      // Now reflow an early line that is above the viewport, while appending one.
      // A differential update can't address rows already in scrollback, so the
      // renderer falls back to a full redraw (matching pi's renderer).
      stdout.clear()
      const reflowed = [...history]
      reflowed[5] = 'hist 5 REFLOWED'
      lines = [...reflowed, 's0', 's1', 's2', 's3', 's4', 's5', 's6']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain(CLEAR_SCREEN)
      // pi rebuilds the full logical frame after clearing the terminal.
      expect(out).toContain('hist 5 REFLOWED')
      expect(out).toContain('s6')
      renderer.destroy()
    })

    test('full redraw keeps the newest content visible', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 6
      renderer.init()
      const history = Array.from({ length: 20 }, (_, i) => `h${i}`)
      let lines = [...history, 'a', 'b', 'c']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      stdout.clear()
      const reflowed = [...history]
      reflowed[0] = 'h0-changed'
      lines = [...reflowed, 'a', 'b', 'c', 'd']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain(CLEAR_SCREEN)
      expect(out).toContain('h0-changed')
      expect(out).toContain('d')
      renderer.destroy()
    })

    // Match pi: any changed line above the addressable viewport forces a full
    // redraw, even when the visible suffix is unchanged.
    test('off-viewport change with unchanged line count triggers full redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      let banner = 'banner: main'
      const history = Array.from({ length: 200 }, (_, i) => `hist ${i}`)
      renderer.setRenderCallback(() => [banner, ...history])
      await renderFrame(renderer)

      // The banner sits at row 0, far above the viewport. Changing it (e.g. a
      // git branch switch or update notice) must not clear the screen.
      stdout.clear()
      banner = 'banner: feature-branch'
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain(CLEAR_SCREEN)
      expect(out).toContain('feature-branch')
      renderer.destroy()
    })

    test('off-viewport early reflow (count unchanged) triggers full redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      const history = Array.from({ length: 8 }, (_, i) => `H${i}`)
      let pending = Array.from({ length: 12 }, (_, i) => `P${i}`)
      renderer.setRenderCallback(() => [...history, ...pending])
      await renderFrame(renderer)

      // Reflow an early pending line that has scrolled above the viewport, with
      // no change to the total line count (in-place table realign / renumber).
      stdout.clear()
      pending = [...pending]
      pending[1] = 'P1-REFLOWED'
      await renderFrame(renderer)

      expect(stdout.output).toContain(CLEAR_SCREEN)
      renderer.destroy()
    })

    test('off-viewport line-count GROWTH with identical viewport redraws like pi', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      let banner = ['banner']
      const history = Array.from({ length: 200 }, (_, i) => `hist ${i}`)
      renderer.setRenderCallback(() => [...banner, ...history])
      await renderFrame(renderer)

      // Banner grows from 1 line to 2 (async update notice). Visible rows are
      // the tail of history, unchanged.
      stdout.clear()
      banner = ['banner', 'New version available']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain(CLEAR_SCREEN)
      expect(out).toContain('New version available')
      renderer.destroy()
    })

    test('off-viewport line-count SHRINK with identical viewport redraws like pi', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      let banner = ['banner', 'transient notice']
      const history = Array.from({ length: 200 }, (_, i) => `hist ${i}`)
      renderer.setRenderCallback(() => [...banner, ...history])
      await renderFrame(renderer)

      stdout.clear()
      banner = ['banner']
      await renderFrame(renderer)

      expect(stdout.output).toContain(CLEAR_SCREEN)
      renderer.destroy()
    })

    test('partial-viewport reach redraws when the first change is above the viewport', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      const history = Array.from({ length: 8 }, (_, i) => `H${i}`)
      // 12 pending: with rows=10 and 20 total lines, viewportTop = 10.
      let pending = Array.from({ length: 12 }, (_, i) => `P${i}`)
      renderer.setRenderCallback(() => [...history, ...pending])
      await renderFrame(renderer)

      // Change a line that spans from above the viewport (index 9, off-screen)
      // to inside it (index 11, visible). Count unchanged.
      stdout.clear()
      pending = [...pending]
      pending[1] = 'P1-off'   // buffer index 9 — above viewport
      pending[3] = 'P3-vis'   // buffer index 11 — inside viewport
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain(CLEAR_SCREEN)
      expect(out).toContain('P1-off')
      expect(out).toContain('P3-vis')
      renderer.destroy()
    })

    // Ctrl+O expand: a tool block in the viewport grows from compact to
    // expanded with the prompt below it. The change starts inside the viewport,
    // so the renderer repaints in place from the first changed row down and
    // scrolls the prompt naturally — no viewport clear, no jump to the top.
    test('expanding in-viewport content grows in place without clearing', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 12
      renderer.init()
      // Small transcript that fits entirely on screen: a couple of history
      // lines, a compact tool card, then the prompt.
      let lines = ['h0', 'h1', 'tool ✓ 2 lines', 'prompt']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      // Ctrl+O expands the tool card into its full output. The card and prompt
      // are all visible, so this must NOT clear the screen.
      stdout.clear()
      lines = ['h0', 'h1', 'tool ✓ 2 lines', '  out line 1', '  out line 2', '  out line 3', 'prompt']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain(CLEAR_SCREEN)
      expect(out).toContain('out line 3')
      expect(out).toContain('prompt')
      renderer.destroy()
    })
  })

  describe('screen overlays', () => {
    test('screen overlay does not inflate a long transcript frame', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 8
      stdout.columns = 40
      renderer.init()
      const history = Array.from({ length: 20 }, (_, index) => `history ${index}`)
      let showOverlay = false
      renderer.setRenderCallback(() => ({
        lines: history,
        ...(showOverlay ? { overlay: { lines: ['Pick model', '❯ model-a', '  model-b'] } } : {}),
      }))
      await renderFrame(renderer)

      stdout.clear()
      showOverlay = true
      await renderFrame(renderer)

      expect(stdout.output).toContain('Pick model')
      expect((renderer as any).previousLines).toHaveLength(history.length)
      renderer.destroy()
    })

    test('screen overlay pads only short frames to one viewport', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 8
      stdout.columns = 40
      renderer.init()
      renderer.setRenderCallback(() => ({
        lines: ['history', 'prompt'],
        overlay: { lines: ['Help', 'close'] },
      }))
      await renderFrame(renderer)

      expect((renderer as any).previousLines).toHaveLength(stdout.rows)
      renderer.destroy()
    })
  })

  describe('clearScreen', () => {
    test('clears screen and resets state', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2'])
      await renderFrame(renderer)
      stdout.clear()
      renderer.clearScreen()
      const out = stdout.output
      expect(out).toContain('\x1b[2J') // clear screen
      expect(out).toContain('\x1b[H')  // cursor home
      expect(out).toContain('\x1b[3J') // pi-style full terminal clear
      renderer.destroy()
    })
  })

  describe('resize handling', () => {
    test('updates dimensions on resize', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.rows = 40
      stdout.columns = 120
      stdout.emit('resize')
      expect(renderer.termRows).toBe(40)
      expect(renderer.termCols).toBe(120)
      renderer.destroy()
    })

    test('redundant resize event does not force a redraw', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2'])
      await renderFrame(renderer)

      stdout.clear()
      stdout.emit('resize')
      await Bun.sleep(20)

      expect(stdout.output).not.toContain('\x1b[2J')
      expect(stdout.output).not.toContain('\x1b[3J')
      renderer.destroy()
    })

    test('actual resize uses pi-style full redraw', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2'])
      await renderFrame(renderer)

      stdout.clear()
      stdout.columns = 120
      stdout.emit('resize')
      await Bun.sleep(20)

      expect(stdout.output).toContain('\x1b[2J\x1b[H\x1b[3J')
      expect(stdout.output).toContain('line1')
      renderer.destroy()
    })

    test('falls back when resize dimensions are non-finite', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.rows = Infinity
      stdout.columns = NaN
      stdout.emit('resize')
      expect(renderer.termRows).toBe(24)
      expect(renderer.termCols).toBe(80)
      renderer.destroy()
    })
  })

  describe('render throttling', () => {
    test('multiple requestRender calls coalesce into one render', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let callCount = 0
      renderer.setRenderCallback(() => {
        callCount++
        return ['frame ' + callCount]
      })
      stdout.clear()
      renderer.requestRender()
      renderer.requestRender()
      renderer.requestRender()
      await new Promise(resolve => process.nextTick(resolve))
      await Bun.sleep(20)
      // Should only have rendered once
      expect(callCount).toBe(1)
      renderer.destroy()
    })
  })

  describe('terminal line safety', () => {
    test('normalizes visible tabs and appends a segment reset to every line', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['left\tright'])
      stdout.clear()
      await renderFrame(renderer)

      expect(stdout.output).not.toContain('\t')
      expect(stdout.output).toContain('left   right\x1b[0m\x1b]8;;\x07')
      renderer.destroy()
    })

    test('pi APC cursor marker is zero-width and removed before paint', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => [`ab${CURSOR_MARKER}cd`])
      stdout.clear()
      await renderFrame(renderer)

      expect(stdout.output).not.toContain(CURSOR_MARKER)
      expect(stripAnsi(stdout.output)).toContain('abcd')
      expect(stdout.output).toContain('\x1b[3G')
      renderer.destroy()
    })

    test('cursor marker parks the hardware cursor but keeps it hidden', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => [`ab${CURSOR_MARKER}cd`])
      stdout.clear()
      await renderFrame(renderer)

      // The column tracks the caret for input methods, but the prompt paints
      // the visible caret itself.
      expect(stdout.output).toContain('\x1b[3G')
      expect(stdout.output).toContain('\x1b[?25l')
      expect(stdout.output).not.toContain('\x1b[?25h')
      renderer.destroy()
    })

    test('lines wider than terminal are clipped by DECAWM off', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.columns = 20
      renderer.init()
      // Verify DECAWM off (no-wrap) is sent on init
      expect(stdout.output).toContain('\x1b[?7l')
      const longLine = 'A'.repeat(50)
      renderer.setRenderCallback(() => [longLine])
      stdout.clear()
      await renderFrame(renderer)
      // The renderer outputs the full line — the terminal clips it
      expect(stdout.output).toContain(longLine)
      renderer.destroy()
    })

    test('destroy re-enables auto-wrap', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.destroy()
      expect(stdout.output).toContain('\x1b[?7h')
    })

    test('lines within terminal width render normally', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.columns = 80
      renderer.init()
      const shortLine = 'Hello world'
      renderer.setRenderCallback(() => [shortLine])
      stdout.clear()
      await renderFrame(renderer)
      expect(stdout.output).toContain(shortLine)
      renderer.destroy()
    })
  })
})
