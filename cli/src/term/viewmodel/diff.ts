/** Responsive patch layout shared by live and completed edit cards. */
import stringWidth from 'string-width'
import stripAnsi from 'strip-ansi'
import { colorizeUnifiedDiffRows, parseDiffHunks } from '../../render/diff.js'
import { wrapTextWithAnsi } from '../../render/wrap.js'
import { getTheme } from '../../render/theme/index.js'
import { line, plain, dim, colored, type StyledLine } from './types.js'

/** Match opencode's inline edit breakpoint; narrower panes retain unified diff. */
export const SPLIT_DIFF_MIN_COLUMNS = 121

type Cell = { number: number; code: string; kind: 'add' | 'remove' | 'context' }

export function buildDiffLines(patch: string, columns?: number): StyledLine[] {
  if (!columns || columns < SPLIT_DIFF_MIN_COLUMNS) {
    const theme = getTheme()
    return colorizeUnifiedDiffRows(patch, false).flatMap(row =>
      wrapTextWithAnsi(row.text, Math.max(1, columns ?? 10000)).map(text => ({
        ...line(plain(text)),
        bg: row.kind === 'add' ? theme.diffAddedBg : row.kind === 'remove' ? theme.diffRemovedBg : undefined,
      })),
    )
  }
  const hunks = parseDiffHunks(patch)
  if (!hunks.length) return wrapTextWithAnsi(patch, columns).map(text => line(plain(text)))
  const leftWidth = Math.floor((columns - 3) / 2)
  const rightWidth = columns - 3 - leftWidth
  const result: StyledLine[] = []
  const cell = (value: Cell | undefined, width: number, gutter: number): StyledLine[] => {
    if (!value) return [line(plain(' '.repeat(width)))]
    const prefix = `${String(value.number).padStart(gutter)} `
    const code = stripAnsi(value.code).replace(/\t/g, '    ').replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '')
    const fragments = wrapTextWithAnsi(code, Math.max(1, width - prefix.length))
    return (fragments.length ? fragments : ['']).map((text, index) => {
      const content = (index === 0 ? prefix : ' '.repeat(prefix.length)) + text
      const padded = content + ' '.repeat(Math.max(0, width - stringWidth(content)))
      const theme = getTheme()
      if (value.kind === 'context') return line(dim(padded))
      const color = value.kind === 'add' ? 'green' : 'red'
      const bg = value.kind === 'add' ? theme.diffAddedBg : theme.diffRemovedBg
      return line({ ...colored(padded, color), bg })
    })
  }
  for (const hunk of hunks) {
    if (result.length) result.push(line(dim('  …')))
    let old = hunk.oldStart
    let next = hunk.newStart
    const gutter = String(Math.max(old + hunk.oldLines, next + hunk.newLines)).length
    const append = (left?: Cell, right?: Cell) => {
      const a = cell(left, leftWidth, gutter)
      const b = cell(right, rightWidth, gutter)
      for (let index = 0; index < Math.max(a.length, b.length); index++) {
        result.push(line(...(a[index]?.spans ?? [plain(' '.repeat(leftWidth))]), dim(' │ '), ...(b[index]?.spans ?? [plain(' '.repeat(rightWidth))])))
      }
    }
    for (let i = 0; i < hunk.lines.length;) {
      const text = hunk.lines[i]!
      if (text.startsWith('\\')) { i++; continue }
      if (text.startsWith(' ')) {
        append({ number: old++, code: text.slice(1), kind: 'context' }, { number: next++, code: text.slice(1), kind: 'context' })
        i++
        continue
      }
      const removed: Cell[] = []
      const added: Cell[] = []
      while (i < hunk.lines.length && !hunk.lines[i]!.startsWith(' ')) {
        const change = hunk.lines[i++]!
        if (change.startsWith('-')) removed.push({ number: old++, code: change.slice(1), kind: 'remove' })
        else if (change.startsWith('+')) added.push({ number: next++, code: change.slice(1), kind: 'add' })
      }
      for (let k = 0; k < Math.max(removed.length, added.length); k++) append(removed[k], added[k])
    }
  }
  return result
}
