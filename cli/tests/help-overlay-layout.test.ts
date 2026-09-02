import { describe, test, expect } from 'bun:test'
import stripAnsi from 'strip-ansi'
import { buildOverlayBlocks } from '../src/term/viewmodel/overlays.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'

/**
 * The help panel is a two-column table. Its readability depends on every
 * command starting in the same column and every description starting in the
 * same column, with only the title and footer centred.
 */
function helpRows(columns = 80): string[] {
  return blocksToLines(buildOverlayBlocks({ kind: 'help' }, columns)).map(stripAnsi)
}

/** Rows listing a command or key binding, i.e. everything but title/footer/blanks. */
function entryRows(rows: string[]): string[] {
  return rows.filter(row => /^\s{2}\S/.test(row) && !row.includes('Press Esc to dismiss'))
}

describe('help overlay layout', () => {
  test('commands all start in the same column', () => {
    const entries = entryRows(helpRows())
    expect(entries.length).toBeGreaterThan(10)
    const indents = new Set(entries.map(row => row.length - row.trimStart().length))
    expect([...indents]).toEqual([2])
  })

  test('descriptions share one column', () => {
    const entries = entryRows(helpRows())
    // Second column begins after the widest key plus the gap, identically on
    // every row — this is what makes the panel scannable.
    const descColumns = new Set(entries.map(row => {
      const afterKey = row.slice(2).search(/\s\s+\S/)
      const gapStart = 2 + afterKey + 1
      return gapStart + row.slice(gapStart).search(/\S/)
    }))
    expect(descColumns.size).toBe(1)
  })

  test('title and dismiss hint are centred over the table, not left-aligned', () => {
    const rows = helpRows()
    const title = rows.find(row => row.includes('Keyboard Shortcuts'))
    const footer = rows.find(row => row.includes('Press Esc to dismiss'))
    expect(title).toBeDefined()
    expect(footer).toBeDefined()

    const blockWidth = Math.max(...rows.map(row => row.length))
    for (const row of [title!, footer!]) {
      const text = row.trim()
      const indent = row.length - row.trimStart().length
      const expected = Math.floor((blockWidth - text.length) / 2)
      // Centring is measured against the table's own width, so it survives the
      // renderer centring the block again inside the viewport.
      expect(Math.abs(indent - expected)).toBeLessThanOrEqual(1)
    }
  })

  test('advertises the /sessions alias next to /resume without showing /restart', () => {
    const rows = helpRows()
    const resumeRow = rows.find(row => row.includes('/resume'))
    expect(resumeRow).toContain('/sessions')
    expect(rows.some(row => row.includes('/restart'))).toBe(false)
  })
})
