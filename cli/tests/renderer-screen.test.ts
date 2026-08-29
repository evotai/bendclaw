import { describe, expect, test } from 'bun:test'
import { TermRenderer, CURSOR_MARKER } from '../src/term/renderer.js'
import { ScreenHarness } from './helpers/screen.js'

async function renderFrame(renderer: TermRenderer): Promise<void> {
  renderer.requestRender()
  await new Promise(resolve => process.nextTick(resolve))
  await Bun.sleep(20)
}

/**
 * These assertions read the physical viewport of a real terminal emulator,
 * which is the only way to tell "row 11 of the screen" from "row 11 of the
 * logical frame".
 *
 * The rule under test: the content above the composer decides where it sits.
 * A short session keeps it high; once output has pushed it to the bottom row it
 * stays there instead of jumping back up when a frame shrinks.
 */
describe('composer placement on a real screen', () => {
  test('a fresh short session keeps the composer just below its content', async () => {
    const screen = new ScreenHarness(80, 24)
    const renderer = new TermRenderer({ stdout: screen.stdout })
    renderer.init()
    renderer.setRenderCallback(() => ({
      lines: ['evot v1', '', '\u276f hi', 'hello there', `\u276f ${CURSOR_MARKER}`, 'footer row'],
      bottomAnchor: true,
    }))

    await renderFrame(renderer)
    await screen.settle()

    // Natural position, not the bottom row: padding a short frame down to the
    // viewport would pin the composer to the bottom of an almost-empty session.
    expect(screen.rowOf('evot v1')).toBe(0)
    expect(screen.rowOf('footer row')).toBe(5)
    expect(screen.viewport()[4]).toBe('\u276f')
    renderer.destroy()
  })

  test('a short session that shrinks follows its content up', async () => {
    const screen = new ScreenHarness(80, 24)
    const renderer = new TermRenderer({ stdout: screen.stdout })
    renderer.init()
    let thinking = ['thinking 0', 'thinking 1', 'thinking 2']
    renderer.setRenderCallback(() => ({
      lines: ['evot v1', '\u276f hi', ...thinking, `\u276f ${CURSOR_MARKER}`, 'footer row'],
      bottomAnchor: true,
    }))
    await renderFrame(renderer)
    await screen.settle()
    expect(screen.rowOf('footer row')).toBe(6)

    thinking = []
    await renderFrame(renderer)
    await screen.settle()

    // The composer was never at the bottom, so there is nothing to hold it
    // down: it rises with the content and leaves no stale rows behind.
    expect(screen.rowOf('footer row')).toBe(3)
    expect(screen.lastNonBlankRow()).toBe(3)
    expect(screen.viewport().some(line => line.startsWith('thinking '))).toBe(false)
    renderer.destroy()
  })

  test('a frame taller than the viewport ends on the bottom row', async () => {
    const screen = new ScreenHarness(80, 24)
    const renderer = new TermRenderer({ stdout: screen.stdout })
    renderer.init()
    const history = Array.from({ length: 60 }, (_, i) => `history ${i}`)
    renderer.setRenderCallback(() => ({
      lines: [...history, `\u276f ${CURSOR_MARKER}`, 'footer row'],
      bottomAnchor: true,
    }))

    await renderFrame(renderer)
    await screen.settle()

    expect(screen.rowOf('footer row')).toBe(screen.rows - 1)
    expect(screen.lastNonBlankRow()).toBe(screen.rows - 1)
    renderer.destroy()
  })

  // The reported regression. The transcript is far taller than the viewport, so
  // the composer has legitimately reached the bottom row. Differential shrink
  // cleared the vacated rows but left the viewport where the taller frame put
  // it, lifting the composer by exactly the number of discarded rows: "however
  // big the last thinking was, that is how far it moves up".
  test.each([1, 3, 8, 12, 30, 80])(
    'discarding %i rows of thinking does not lift the composer off the bottom row',
    async thinkingRows => {
      const screen = new ScreenHarness(80, 24)
      const renderer = new TermRenderer({ stdout: screen.stdout })
      renderer.init()
      const transcript = Array.from({ length: 40 }, (_, i) => `history ${i}`)
      let thinking = Array.from({ length: thinkingRows }, (_, i) => `thinking ${i}`)
      renderer.setRenderCallback(() => ({
        lines: [...transcript, ...thinking, `\u276f ${CURSOR_MARKER}`, 'footer row'],
        bottomAnchor: true,
      }))
      await renderFrame(renderer)
      await screen.settle()
      expect(screen.rowOf('footer row')).toBe(screen.rows - 1)

      thinking = []
      await renderFrame(renderer)
      await screen.settle()

      const viewport = screen.viewport()
      expect(screen.rowOf('footer row')).toBe(screen.rows - 1)
      expect(viewport[screen.rows - 2]).toBe('\u276f')
      // No stale thinking rows survive the interrupt.
      expect(viewport.some(line => line.startsWith('thinking '))).toBe(false)
      renderer.destroy()
    },
  )

  test('repeated interrupts keep the composer on the same row', async () => {
    const screen = new ScreenHarness(80, 24)
    const renderer = new TermRenderer({ stdout: screen.stdout })
    renderer.init()
    const transcript = Array.from({ length: 40 }, (_, i) => `history ${i}`)
    let thinking: string[] = []
    renderer.setRenderCallback(() => ({
      lines: [...transcript, ...thinking, `\u276f ${CURSOR_MARKER}`, 'footer row'],
      bottomAnchor: true,
    }))

    const rows: number[] = []
    for (const height of [6, 14, 3, 21, 9]) {
      thinking = Array.from({ length: height }, (_, i) => `thinking ${i}`)
      await renderFrame(renderer)
      thinking = []
      await renderFrame(renderer)
      await screen.settle()
      rows.push(screen.rowOf('footer row'))
    }

    // Every interrupt lands on the same row regardless of how tall the
    // discarded block was: no drift accumulates across turns.
    expect(rows).toEqual([23, 23, 23, 23, 23])
    renderer.destroy()
  })

  test('streaming growth stays on the differential path and keeps scrollback', async () => {
    const screen = new ScreenHarness(80, 24)
    const branches: string[] = []
    const renderer = new TermRenderer({
      stdout: screen.stdout,
      trace: entry => branches.push(entry.branch),
    })
    renderer.init()
    const transcript = Array.from({ length: 40 }, (_, i) => `history ${i}`)
    let thinking: string[] = []
    renderer.setRenderCallback(() => ({
      lines: [...transcript, ...thinking, `\u276f ${CURSOR_MARKER}`, 'footer row'],
      bottomAnchor: true,
    }))
    await renderFrame(renderer)

    branches.length = 0
    for (let rows = 1; rows <= 25; rows++) {
      thinking = Array.from({ length: rows }, (_, i) => `thinking ${i}`)
      await renderFrame(renderer)
    }
    // Re-anchoring must not turn the streaming hot path into full repaints.
    expect(branches.every(b => b === 'differential_update' || b === 'no_change')).toBe(true)

    thinking = []
    await renderFrame(renderer)
    await screen.settle()
    expect(screen.rowOf('footer row')).toBe(screen.rows - 1)

    // The repaint re-emits the transcript rather than duplicating or losing it.
    const buffer = screen.terminal.buffer.active
    const allRows: string[] = []
    for (let row = 0; row < buffer.length; row++) {
      allRows.push((buffer.getLine(row)?.translateToString(true) ?? '').trimEnd())
    }
    expect(allRows.filter(line => line === 'history 0')).toHaveLength(1)
    expect(allRows).toContain('history 39')
    renderer.destroy()
  })
})
