import { beforeAll, describe, expect, test } from 'bun:test'
import chalk from 'chalk'
import stringWidth from 'string-width'
import stripAnsi from 'strip-ansi'
import { getTheme, resetThemeCache } from '../src/render/theme.js'
import { visibleWidth } from '../src/render/wrap.js'
import { CURSOR_MARKER } from '../src/term/renderer.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { joinLeftRight, spansWidth } from '../src/term/viewmodel/width.js'
import { buildPromptBlocks, type PromptVMInput } from '../src/term/viewmodel/prompt.js'
import { buildPromptFooterBlocks } from '../src/term/viewmodel/prompt-footer.js'

beforeAll(() => { chalk.level = 3 })

function defaultInput(overrides: Partial<PromptVMInput> = {}): PromptVMInput {
  return {
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    active: true,
    caretVisible: true,
    completion: null,
    ghostHint: '',
    columns: 80,
    rows: 24,
    placeholder: true,
    model: 'claude-sonnet',
    provider: '',
    thinkingLevel: '',
    planning: false,
    logMode: false,
    dashboardUrl: null,
    exitHint: false,
    cwd: '/Users/test/project',
    gitBranch: 'main',
    contextTokens: 0,
    contextWindow: 0,
    backgroundProcessCount: 0,
    backgroundPanelDownAvailable: false,
    ...overrides,
  }
}

function render(input: PromptVMInput): string {
  return blocksToLines(buildPromptBlocks(input)).join('\n')
}

function renderPlain(input: PromptVMInput): string {
  return stripAnsi(render(input)).replaceAll(CURSOR_MARKER, '')
}

function renderLines(input: PromptVMInput): string[] {
  return blocksToLines(buildPromptBlocks(input))
}

/** Rows that belong to the input frame (borders and railed content). */
function frameLines(input: PromptVMInput): string[] {
  return renderLines(input).filter(row => /^[│╭╰─]/.test(stripAnsi(row)))
}

function menuOf(count: number, selectedIndex: number) {
  return {
    items: Array.from({ length: count }, (_, index) => ({
      label: `/cmd${index}`,
      value: `/cmd${index} `,
      description: `does thing ${index}`,
    })),
    selectedIndex,
    replaceStart: 0,
    replaceEnd: 3,
  }
}

/** Candidate labels currently inside the completion viewport, in order. */
function visibleCandidates(input: PromptVMInput): string[] {
  return renderLines(input).flatMap(row => {
    const match = /(\/cmd\d+)/.exec(stripAnsi(row))
    return match ? [match[1]!] : []
  })
}

function completion(labels: string[], selectedIndex = 0) {
  return {
    items: labels.map(label => ({ label, value: `${label} `, description: `Description for ${label}` })),
    selectedIndex,
    replaceStart: 0,
    replaceEnd: 2,
  }
}

describe('prompt editor', () => {
  test('renders border, caret and placeholder', () => {
    const ansi = render(defaultInput())
    const plain = stripAnsi(ansi).replaceAll(CURSOR_MARKER, '')
    expect(plain).toContain(`╭${'─'.repeat(78)}╮`)
    expect(plain).toContain(`╰${'─'.repeat(78)}╯`)
    expect(plain).toContain('▍')
    expect(plain).toContain('Enter a coding task or / for commands')
    // The end-of-line caret carries the cursor hue, not inverse video.
    expect(ansi).toContain(chalk.bold(chalk.hex('#9ae65c')('▍')))
  })

  test('uses the theme-aware EVOT brand color for both input borders', () => {
    const previousTheme = process.env.EVOT_THEME
    try {
      for (const [scheme, hex] of [['dark', '#b5bcf9'], ['light', '#5769f7']] as const) {
        process.env.EVOT_THEME = scheme
        resetThemeCache()
        const lines = render(defaultInput()).split('\n')
        for (const border of [`╭${'─'.repeat(78)}╮`, `╰${'─'.repeat(78)}╯`]) {
          expect(lines.filter(line => line === chalk.hex(hex)(border))).toHaveLength(1)
        }
        // The side rails carry the same brand hue as the corners.
        expect(lines.some(line => line.startsWith(chalk.hex(hex)('│')))).toBe(true)
      }
    } finally {
      if (previousTheme === undefined) delete process.env.EVOT_THEME
      else process.env.EVOT_THEME = previousTheme
      resetThemeCache()
    }
  })

  test('renders input and known command styling', () => {
    const input = defaultInput({ lines: ['/plan remove unwraps'], cursorCol: 5, placeholder: false })
    // The cursor is mid-line, so the character it sits on becomes a block; the
    // rest of the text is contiguous.
    expect(renderPlain(input)).toContain('/plan remove unwraps')
    // Known commands share the frame's brand hue rather than a fixed ANSI cyan.
    expect(render(input)).toContain(chalk.bold(chalk.hex('#b5bcf9')('/plan')))
  })

  test('does not style unknown slash text as a command', () => {
    const ansi = render(defaultInput({ lines: ['/unknown text'], cursorCol: 8, placeholder: false }))
    expect(ansi).not.toContain(chalk.bold(chalk.hex('#b5bcf9')('/unknown')))
  })

  test('wraps ASCII and CJK input within terminal width', () => {
    for (const text of ['a'.repeat(50), '改进不过测试一定要在目录']) {
      const plain = renderPlain(defaultInput({ columns: 20, lines: [text], cursorCol: text.length, placeholder: false }))
      for (const row of plain.split('\n')) expect(stringWidth(row)).toBeLessThanOrEqual(20)
    }
  })

  test('puts an end caret on a fresh row when the previous row is full', () => {
    // At 20 columns the frame degrades, so text gets all 20: a 20-character
    // draft fills the row exactly and the caret has to wrap.
    const ansi = render(defaultInput({ columns: 20, lines: ['a'.repeat(20)], cursorCol: 20, placeholder: false }))
    const rows = stripAnsi(ansi).replaceAll(CURSOR_MARKER, '').split('\n')
      .filter(row => /^a+$/.test(row) || row === '▍')
    expect(rows).toEqual(['a'.repeat(20), '▍'])
  })

  test('limits long input to 30 percent of terminal rows and follows the cursor', () => {
    const lines = Array.from({ length: 12 }, (_, index) => `line ${index + 1}`)
    const plain = renderPlain(defaultInput({
      lines,
      cursorLine: 11,
      cursorCol: lines[11]!.length,
      rows: 20,
      placeholder: false,
    }))
    expect(plain).toContain('↑ 6 lines')
    expect(plain).not.toContain('line 1\n')
    expect(plain).toContain('line 12')
  })

  test('shows lines below when the cursor is near the top', () => {
    const lines = Array.from({ length: 10 }, (_, index) => `row ${index + 1}`)
    const plain = renderPlain(defaultInput({ lines, cursorLine: 0, cursorCol: 0, rows: 20, placeholder: false }))
    expect(plain).toContain('↓ 4 lines')
    expect(plain).toContain('row 1')
    expect(plain).not.toContain('row 10')
  })

  test('places the caret before the ghost hint without inserting a blank cell', () => {
    const input = defaultInput({ lines: ['/mo'], cursorCol: 3, placeholder: false, ghostHint: 'del  [<name>]' })
    const ansi = render(input)
    // Cursor is at end of line, so the thin bar precedes the ghost suffix with
    // no blank cell between them: `/mo▍del  [<name>]`.
    expect(stripAnsi(ansi).replaceAll(CURSOR_MARKER, '')).toContain('/mo▍del  [<name>]')
    expect(ansi).toContain(chalk.bold(chalk.hex('#9ae65c')('▍')))
  })

  test('renders a caret at end of line without a ghost hint', () => {
    const ansi = render(defaultInput({ lines: ['/mo'], cursorCol: 3, placeholder: false }))
    expect(stripAnsi(ansi).replaceAll(CURSOR_MARKER, '')).toContain('/mo▍')
  })

  test('blanks the end-of-line caret on the dark half of the blink cycle', () => {
    const input = { lines: ['/mo'], cursorCol: 3, placeholder: false }
    const off = render(defaultInput({ ...input, caretVisible: false }))
    const plain = stripAnsi(off).replaceAll(CURSOR_MARKER, '')
    expect(plain).not.toContain('▍')
    expect(off).not.toContain(chalk.bold(chalk.hex('#9ae65c')('▍')))
    // A space still claims the caret column, so the row keeps its width.
    const onRow = renderPlain(defaultInput({ ...input, caretVisible: true }))
      .split('\n').find(row => row.includes('▍'))
    const offRow = plain.split('\n').find(row => row.includes('/mo'))
    expect(offRow).toBeDefined()
    expect(offRow?.length).toBe(onRow?.length)
  })

  test('blanks the placeholder caret while the hint stays put', () => {
    const hint = 'Enter a coding task or / for commands'
    const off = renderPlain(defaultInput({ caretVisible: false }))
    expect(off).not.toContain('▍')
    const offRow = off.split('\n').find(row => row.includes(hint))
    const onRow = renderPlain(defaultInput()).split('\n').find(row => row.includes(hint))
    expect(offRow).toBeDefined()
    expect(offRow?.indexOf(hint)).toBe(onRow?.indexOf(hint) ?? -1)
    expect(offRow?.length).toBe(onRow?.length)
  })

  test('keeps the on-character cursor block solid across the blink cycle', () => {
    const input = { lines: ['/model '], cursorCol: 6, placeholder: false }
    const on = render(defaultInput({ ...input, caretVisible: true }))
    const off = render(defaultInput({ ...input, caretVisible: false }))
    expect(off).toContain('\x1b[48;2;154;230;92m')
    expect(off).toBe(on)
  })

  test('highlights the character under the cursor when it is not at end of line', () => {
    const input = defaultInput({ lines: ['/model '], cursorCol: 6, placeholder: false, ghostHint: '[<name>]' })
    const ansi = render(input)
    // The cursor sits on the trailing space, which becomes a block; the text
    // stays contiguous rather than being split by an inserted bar.
    expect(stripAnsi(ansi).replaceAll(CURSOR_MARKER, '')).toContain('/model [<name>]')
    expect(ansi).toContain('\x1b[48;2;154;230;92m')
  })

  test('renders a five-row completion viewport with descriptions and position', () => {
    const plain = renderPlain(defaultInput({ completion: completion(['/a', '/b', '/c', '/d', '/e', '/f'], 5) }))
    expect(plain).not.toContain('/a')
    expect(plain).toContain('/f')
    expect(plain).toContain('Description for /f')
    expect(plain).toContain('6/6')
  })

  test('keeps completion rows within terminal width', () => {
    const plain = renderPlain(defaultInput({
      columns: 24,
      completion: completion(['/very-long-command-one', '/very-long-command-two']),
    }))
    for (const row of plain.split('\n')) expect(stringWidth(row)).toBeLessThanOrEqual(24)
  })

  test('preserves prompt spacing and attached layout', () => {
    expect(buildPromptBlocks(defaultInput())[0]!.marginTop).toBe(1)
    expect(buildPromptBlocks(defaultInput(), { attachedAbove: true })[0]!.marginTop).toBe(0)
  })


  test('shows exit hint', () => {
    expect(renderPlain(defaultInput({ exitHint: true }))).toContain('Press Ctrl+C again to exit')
  })

  test('uses fallback dimensions for non-finite terminal sizes', () => {
    const plain = renderPlain(defaultInput({ columns: Infinity, rows: Infinity }))
    expect(plain.split('\n')).toContain(`╭${'─'.repeat(78)}╮`)
  })
})

describe('prompt frame', () => {
  const states: [string, Partial<PromptVMInput>][] = [
    ['empty placeholder', {}],
    ['single line', { lines: ['fix the retry backoff'], cursorCol: 21, placeholder: false }],
    ['ghost hint', { lines: ['/model '], cursorCol: 7, placeholder: false, ghostHint: '[<name>]' }],
    ['completion menu', { lines: ['/cm'], cursorCol: 3, placeholder: false, completion: menuOf(6, 0) }],
    ['exit hint', { exitHint: true }],
    ['multiline overflow', {
      placeholder: false,
      rows: 20,
      lines: Array.from({ length: 20 }, (_, index) => `line ${index + 1}`),
      cursorLine: 11,
      cursorCol: 7,
    }],
    ['CJK wrap', { placeholder: false, lines: ['帮我把输入框换成圆角盒子并让选中行铺满整行'], cursorCol: 21 }],
  ]

  test.each(states)('pads every framed row to the exact terminal width: %s', (_label, overrides) => {
    for (const columns of [30, 48, 80, 120]) {
      for (const row of frameLines(defaultInput({ columns, ...overrides }))) {
        expect(visibleWidth(row)).toBe(columns)
      }
    }
  })

  test.each(states)('never overflows the terminal width: %s', (_label, overrides) => {
    for (const columns of [12, 20, 29, 30, 48, 80]) {
      for (const row of renderLines(defaultInput({ columns, ...overrides }))) {
        expect(visibleWidth(row)).toBeLessThanOrEqual(columns)
      }
    }
  })

  test('draws rounded corners and side rails', () => {
    const plain = frameLines(defaultInput({ columns: 40 })).map(stripAnsi)
    expect(plain[0]).toBe(`╭${'─'.repeat(38)}╮`)
    expect(plain[plain.length - 1]).toBe(`╰${'─'.repeat(38)}╯`)
    expect(plain[1]!.startsWith('│ ')).toBe(true)
    expect(plain[1]!.endsWith(' │')).toBe(true)
  })

  test('widens the gutter as the terminal grows', () => {
    // Breathing room costs columns, not rows, so it scales with width and
    // shrinks back before content has to give anything up. Measured on the
    // caret row: centring puts blank rows above it.
    const gutterAt = (columns: number) => {
      const row = frameLines(defaultInput({ columns })).map(stripAnsi).find(line => line.includes('▍'))
      return /^│( *)/.exec(row!)![1]!.length
    }
    expect(gutterAt(40)).toBe(1)
    expect(gutterAt(59)).toBe(1)
    expect(gutterAt(60)).toBe(2)
    expect(gutterAt(99)).toBe(2)
    expect(gutterAt(100)).toBe(3)
    expect(gutterAt(200)).toBe(3)
  })

  test('mirrors the gutter on both rails', () => {
    // Trailing blanks are content padding plus gutter, so the two are
    // indistinguishable until the content fills its width. Overflowing input
    // gets truncated to exactly `contentWidth`, leaving only the gutter. The
    // cursor sits at column 0, so the row carries a block rather than a bar.
    for (const columns of [40, 60, 100]) {
      const row = frameLines(defaultInput({
        columns,
        placeholder: false,
        lines: ['x'.repeat(columns * 2)],
        cursorCol: 0,
      })).map(stripAnsi).find(line => line.includes('x'))!
      const left = /^│( *)/.exec(row)![1]!.length
      const right = /( *)│$/.exec(row)![1]!.length
      expect(right).toBe(left)
      expect(left).toBeGreaterThan(0)
    }
  })

  test('gives the full width back to content once degraded', () => {
    // No rails below the threshold, so no gutter either: the caret leads.
    const row = renderPlain(defaultInput({ columns: 29 })).replaceAll(CURSOR_MARKER, '')
      .split('\n').find(line => line.includes('▍'))
    expect(row!.startsWith('▍')).toBe(true)
  })

  test('keeps a blank-row floor under a short draft', () => {
    // Rows between the rails, borders excluded.
    const railRows = (o: Partial<PromptVMInput>) =>
      frameLines(defaultInput(o)).filter(row => stripAnsi(row).startsWith('│')).length

    expect(railRows({})).toBe(3)
    expect(railRows({ lines: ['one line'], cursorCol: 8, placeholder: false })).toBe(3)
    expect(railRows({ lines: ['a', 'b'], cursorLine: 1, cursorCol: 1, placeholder: false })).toBe(3)
    // Once the draft reaches the floor the composer grows with the content.
    expect(railRows({ lines: ['a', 'b', 'c'], cursorLine: 2, cursorCol: 1, placeholder: false })).toBe(3)
    expect(railRows({ lines: ['a', 'b', 'c', 'd'], cursorLine: 3, cursorCol: 1, placeholder: false })).toBe(4)
  })

  test('centres a one-line draft between the rails', () => {
    // Blank rows above and below the draft, borders excluded.
    const padding = (o: Partial<PromptVMInput>) => {
      const rail = frameLines(defaultInput(o)).map(stripAnsi).filter(row => row.startsWith('│'))
      const isBlank = (row: string) => /^│\s*│$/.test(row)
      const first = rail.findIndex(row => !isBlank(row))
      const last = rail.findLastIndex(row => !isBlank(row))
      return { above: first, below: rail.length - 1 - last }
    }
    // Equal blanks on both sides. This is what forces an odd interior: a
    // 2-row composer would have to put its one spare row on a single side.
    expect(padding({})).toEqual({ above: 1, below: 1 })
    expect(padding({ lines: ['one line'], cursorCol: 8, placeholder: false })).toEqual({ above: 1, below: 1 })
    // Two lines leave one spare row, which goes below.
    expect(padding({ lines: ['a', 'b'], cursorLine: 1, cursorCol: 1, placeholder: false })).toEqual({ above: 0, below: 1 })
    expect(padding({ lines: ['a', 'b', 'c'], cursorLine: 2, cursorCol: 1, placeholder: false })).toEqual({ above: 0, below: 0 })
  })

  test('drops the floor on terminals too short to spare the rows', () => {
    const railRows = (rows: number) =>
      frameLines(defaultInput({ rows })).filter(row => stripAnsi(row).startsWith('│')).length
    expect(railRows(19)).toBe(1)
    expect(railRows(20)).toBe(3)
  })

  test('lets the completion menu absorb the height instead of stacking', () => {
    // Without this the floor's blanks and the menu both apply, pushing the
    // candidates several rows away from what was typed.
    const rows = renderLines(defaultInput({
      lines: ['/cm'],
      cursorCol: 3,
      placeholder: false,
      completion: menuOf(3, 0),
    })).map(stripAnsi)
    const input = rows.findIndex(row => row.includes('/cm'))
    const firstCandidate = rows.findIndex(row => row.includes('/cmd0'))
    // Exactly one separator row between the input and the first candidate.
    expect(firstCandidate - input).toBe(2)
  })

  test('adds no bare blank rows when the frame degrades', () => {
    // The blanks only read as composer space between rails; without them they
    // are indistinguishable from stray whitespace.
    const rows = renderPlain(defaultInput({ columns: 29 })).replaceAll(CURSOR_MARKER, '').split('\n')
    const caretIndex = rows.findIndex(row => row.includes('▍'))
    expect(rows[caretIndex + 1]).toBe('─'.repeat(29))
  })

  test('keeps overflow markers on the frame in their existing wording', () => {
    const plain = renderPlain(defaultInput({
      placeholder: false,
      rows: 20,
      lines: Array.from({ length: 20 }, (_, index) => `line ${index + 1}`),
      cursorLine: 11,
      cursorCol: 7,
    }))
    expect(plain).toContain('╭─ ↑ 6 lines ─')
    expect(plain).toContain('╰─ ↓ 8 lines ─')
  })

  test('truncates a frame label that cannot fit beside the corners', () => {
    for (const row of frameLines(defaultInput({
      columns: 30,
      rows: 12,
      placeholder: false,
      lines: Array.from({ length: 40 }, (_, index) => `line ${index + 1}`),
      cursorLine: 39,
      cursorCol: 7,
    }))) {
      expect(visibleWidth(row)).toBe(30)
    }
  })

  test('degrades to plain rules below the minimum framed width', () => {
    const plain = renderPlain(defaultInput({ columns: 29 })).split('\n')
    expect(plain).toContain('─'.repeat(29))
    // No corners or rails on the input rows. The footer keeps its own `│`
    // separator, so only the frame region is inspected here.
    for (const row of plain.filter(line => /^[│╭╰─]/.test(line))) {
      expect(row).toBe('─'.repeat(29))
    }
  })

  test('spends no width on rails once degraded', () => {
    // The full 29 columns stay available to content, unlike the framed path
    // which reserves four for the rails and their padding.
    const wide = renderPlain(defaultInput({ columns: 29, placeholder: false, lines: ['a'.repeat(40)], cursorCol: 40 }))
    expect(wide).toContain('a'.repeat(27))
  })

  test('places the cursor marker inside the rails', () => {
    const row = renderLines(defaultInput({ columns: 40 })).find(line => line.includes(CURSOR_MARKER))
    expect(row).toBeDefined()
    // Everything before the marker is measurable, so the renderer can derive
    // the hardware cursor column. The marker leads the caret, so at 40 columns
    // only the rail and its one-column gutter sit ahead of it.
    const prefix = row!.slice(0, row!.indexOf(CURSOR_MARKER))
    expect(visibleWidth(prefix)).toBe(2)
  })

  test('drops the border entirely on a terminal too short to spare the rows', () => {
    // At 9 rows the two border rows plus footer would leave the transcript
    // nothing, so the composer keeps only its caret row.
    const plain = renderPlain(defaultInput({ rows: 9 })).split('\n')
    for (const glyph of ['╭', '╮', '╰', '╯', '│']) {
      expect(plain.filter(row => row.startsWith(glyph))).toEqual([])
    }
    expect(plain.some(row => row.includes('▍'))).toBe(true)
  })

  test('keeps the border down to the height where it still pays', () => {
    const plain = renderPlain(defaultInput({ rows: 10 }))
    expect(plain).toContain('╭')
    expect(plain).toContain('╰')
  })

  test('reclaims rows as the terminal shortens rather than crowding out history', () => {
    const height = (rows: number) => renderLines(defaultInput({ rows })).length
    // Each step down sheds chrome: blank filler rows first, then the border.
    expect(height(40)).toBe(8)
    expect(height(14)).toBe(6)
    expect(height(9)).toBe(4)
    // A 9-row terminal must keep most of itself for the transcript.
    expect(height(9)).toBeLessThan(9 / 2 + 1)
  })

  test('recolours both rails and corners for an active mode', () => {
    const { accentHex, brandHex } = getTheme()
    // A border row is painted as one span, so the hue is asserted on the row
    // rather than on the corner glyph alone.
    const rowsFor = (input: PromptVMInput) =>
      renderLines(input).filter(row => /^\x1b\[[\d;]+m[╭╰│]/.test(row))
    const hueOf = (row: string) => row.slice(0, row.indexOf('m') + 1)

    const planned = rowsFor(defaultInput({ planning: true }))
    expect(planned.length).toBeGreaterThan(0)
    for (const row of planned) {
      expect(hueOf(row)).toBe(chalk.hex(accentHex)('x').slice(0, -'x\x1b[39m'.length))
    }
    // The default mode keeps the brand hue.
    for (const row of rowsFor(defaultInput())) {
      expect(hueOf(row)).toBe(chalk.hex(brandHex)('x').slice(0, -'x\x1b[39m'.length))
    }
  })

  test('names the mode in the border label, not just by hue', () => {
    // Colour alone fails on monochrome terminals and for colour-blind users.
    expect(renderPlain(defaultInput({ planning: true }))).toContain('╭─ plan ')
    expect(renderPlain(defaultInput({ logMode: true }))).toContain('╭─ log ')
  })

  test('pushes an unfocused draft into the background', () => {
    const draft = { lines: ['fix the retry backoff'], cursorCol: 21, placeholder: false }
    const focused = render(defaultInput({ ...draft, active: true }))
    const blurred = render(defaultInput({ ...draft, active: false }))

    expect(blurred).not.toBe(focused)
    // The text survives; only its weight changes. `dim` paints as a muted hex
    // rather than SGR 2, so the assertion follows the renderer.
    expect(stripAnsi(blurred)).toContain('fix the retry backoff')
    expect(blurred).toContain(chalk.hex('#777777')('fix the retry backoff'))
    expect(focused).not.toContain(chalk.hex('#777777')('fix the retry backoff'))
  })

  test('drops hue and weight from an unfocused command, not just brightness', () => {
    // A bold brand-coloured command stays loud under dim alone, and that is
    // exactly the text that should recede while a modal owns the screen.
    const { brandHex } = getTheme()
    const command = { lines: ['/model'], cursorCol: 6, placeholder: false }
    expect(render(defaultInput({ ...command, active: true }))).toContain(chalk.hex(brandHex)('/model'))

    const blurred = render(defaultInput({ ...command, active: false }))
    expect(blurred).not.toContain(chalk.hex(brandHex)('/model'))
    expect(blurred).not.toContain('\x1b[1m')
    expect(stripAnsi(blurred)).toContain('/model')
  })

  test('hides the caret entirely while unfocused', () => {
    const draft = { lines: ['draft'], cursorCol: 5, placeholder: false }
    expect(renderPlain(defaultInput({ ...draft, active: true }))).toContain('▍')
    expect(renderPlain(defaultInput({ ...draft, active: false }))).not.toContain('▍')
  })

  test('shares the label slot between mode and scroll overflow', () => {
    const plain = renderPlain(defaultInput({
      columns: 60,
      rows: 20,
      planning: true,
      placeholder: false,
      lines: Array.from({ length: 12 }, (_, index) => `line ${index + 1}`),
      cursorLine: 11,
      cursorCol: 7,
    }))
    // Mode leads: it says what enter will do, overflow only says where you are.
    expect(plain).toContain('╭─ plan · ↑ ')
  })
})

describe('prompt completion menu', () => {
  test('fills the selected row to the right rail', () => {
    const rows = renderLines(defaultInput({
      columns: 60,
      lines: ['/cm'],
      cursorCol: 3,
      placeholder: false,
      completion: menuOf(6, 1),
    }))
    const selected = rows.find(row => stripAnsi(row).includes('/cmd1'))
    expect(selected).toBeDefined()
    // Only the closing rail may follow the final background reset.
    const tail = stripAnsi(selected!.slice(selected!.lastIndexOf('\x1b[49m')))
    expect(tail).toBe('│')

    const unselected = rows.find(row => stripAnsi(row).includes('/cmd2'))
    expect(unselected).not.toContain('\x1b[49m')
  })

  test('uses theme-aware selection colors', () => {
    const previousTheme = process.env.EVOT_THEME
    try {
      for (const [scheme, bgHex] of [['dark', '#2c2f4a'], ['light', '#dfe3fd']] as const) {
        process.env.EVOT_THEME = scheme
        resetThemeCache()
        const rows = renderLines(defaultInput({
          lines: ['/cm'],
          cursorCol: 3,
          placeholder: false,
          completion: menuOf(3, 0),
        }))
        const selected = rows.find(row => stripAnsi(row).includes('/cmd0'))
        expect(selected).toContain(chalk.bgHex(bgHex)('').split('\x1b[49m')[0])
      }
    } finally {
      if (previousTheme === undefined) delete process.env.EVOT_THEME
      else process.env.EVOT_THEME = previousTheme
      resetThemeCache()
    }
  })

  test('keeps the selection near the middle of the viewport', () => {
    const input = (selectedIndex: number) => defaultInput({
      rows: 20,
      lines: ['/cm'],
      cursorCol: 3,
      placeholder: false,
      completion: menuOf(20, selectedIndex),
    })
    expect(visibleCandidates(input(10))).toEqual(['/cmd8', '/cmd9', '/cmd10', '/cmd11', '/cmd12'])
    // Near either end the viewport stops sliding rather than showing blanks.
    expect(visibleCandidates(input(0))).toEqual(['/cmd0', '/cmd1', '/cmd2', '/cmd3', '/cmd4'])
    expect(visibleCandidates(input(19))).toEqual(['/cmd15', '/cmd16', '/cmd17', '/cmd18', '/cmd19'])
  })

  test('shows more candidates on taller terminals', () => {
    const at = (rows: number) => visibleCandidates(defaultInput({
      rows,
      lines: ['/cm'],
      cursorCol: 3,
      placeholder: false,
      completion: menuOf(20, 0),
    })).length
    expect(at(24)).toBe(5)
    expect(at(40)).toBe(12)
  })

  test('shows the position counter only when candidates are hidden', () => {
    const hasCounter = (count: number, rows: number) => renderPlain(defaultInput({
      rows,
      lines: ['/cm'],
      cursorCol: 3,
      placeholder: false,
      completion: menuOf(count, 0),
    })).includes(`1/${count}`)
    expect(hasCounter(5, 24)).toBe(false)
    expect(hasCounter(6, 24)).toBe(true)
    expect(hasCounter(12, 40)).toBe(false)
    expect(hasCounter(13, 40)).toBe(true)
  })

  test('renders an advisory note below candidates and their counter', () => {
    const note = 'files up to 6 levels deep — install fd to search deeper'
    const lines = renderPlain(defaultInput({
      rows: 24,
      lines: ['@src'],
      cursorCol: 4,
      placeholder: false,
      completion: { ...menuOf(6, 0), note },
    })).split('\n')

    const counter = lines.findIndex(row => row.includes('1/6'))
    const advisory = lines.findIndex(row => row.includes(note))
    expect(counter).toBeGreaterThan(-1)
    expect(advisory).toBe(counter + 1)
  })

  test('renders a note-only empty result without a fake candidate', () => {
    const note = 'no matches · files up to 6 levels deep'
    const plain = renderPlain(defaultInput({
      lines: ['@deep'],
      cursorCol: 5,
      placeholder: false,
      completion: {
        items: [],
        selectedIndex: 0,
        replaceStart: 0,
        replaceEnd: 5,
        note,
      },
    }))
    expect(plain).toContain(note)
    expect(plain).not.toContain('❯')
    expect(plain).not.toMatch(/\d+\/0/)
  })

  test('truncates a completion note to the menu width', () => {
    const columns = 32
    const note = 'files up to 6 levels deep — install fd to search deeper'
    const rows = renderLines(defaultInput({
      columns,
      lines: ['@src'],
      cursorCol: 4,
      placeholder: false,
      completion: { ...menuOf(2, 0), note },
    }))
    const advisory = rows.find(row => stripAnsi(row).includes('files up to'))
    expect(advisory).toBeDefined()
    expect(stripAnsi(advisory!)).toContain('…')
    expect(visibleWidth(advisory!)).toBeLessThanOrEqual(columns)
  })

  test('keeps the file name when a completion path is wider than the label column', () => {
    const path = 'src/app/[language]/(home)/gallery/components/GalleryCard.tsx'
    const plain = renderPlain(defaultInput({
      columns: 40,
      lines: ['@GalleryCard'],
      cursorCol: 12,
      placeholder: false,
      completion: {
        items: [{ label: path, value: `@${path} ` }],
        selectedIndex: 0,
        replaceStart: 0,
        replaceEnd: 12,
      },
    }))
    expect(plain).toContain('GalleryCard.tsx')
    expect(plain).toContain('…')
    expect(plain).not.toContain('src/app/[language]')
  })

  test('gives file candidates the full row when they have no description', () => {
    const path = 'src/app/[language]/(home)/gallery/components/GalleryCard.tsx'
    const at = (columns: number) => renderPlain(defaultInput({
      columns,
      lines: ['@GalleryCard'],
      cursorCol: 12,
      placeholder: false,
      completion: {
        items: [{ label: path, value: `@${path} ` }],
        selectedIndex: 0,
        replaceStart: 0,
        replaceEnd: 12,
      },
    }))
    // 80 columns is enough for the whole path once the 45% description reserve
    // is released; 40 columns still has to keep the leaf and drop the head.
    expect(at(80)).toContain(path)
    expect(at(80)).not.toContain('…')
    expect(at(40)).toContain('GalleryCard.tsx')
    expect(at(40)).toContain('…')
  })

  test('shrinks candidates so the prompt never exceeds a short terminal', () => {
    const note = 'files up to 6 levels deep — install fd to search deeper'
    for (const rows of [5, 6, 9, 10, 12, 14]) {
      for (const backgroundProcessCount of [0, 1]) {
        const rendered = renderLines(defaultInput({
          rows,
          lines: ['@src'],
          cursorCol: 4,
          placeholder: false,
          backgroundProcessCount,
          completion: { ...menuOf(20, 0), note },
        }))
        expect(rendered.length).toBeLessThanOrEqual(rows)
      }
    }
  })

  test('budgets spinner and queue rows before choosing candidate count', () => {
    const rows = 10
    const prompt = blocksToLines(buildPromptBlocks(defaultInput({
      rows,
      lines: ['@src'],
      cursorCol: 4,
      placeholder: false,
      completion: {
        ...menuOf(20, 0),
        note: 'files up to 6 levels deep — install fd to search deeper',
      },
    }), {
      attachedAbove: true,
      reservedAboveRows: 2,
    }))
    expect(prompt.length + 2).toBeLessThanOrEqual(rows)
  })

  test('separates the input from candidates only while framed', () => {
    const blankRows = (columns: number) => renderLines(defaultInput({
      columns,
      lines: ['/cm'],
      cursorCol: 3,
      placeholder: false,
      completion: menuOf(3, 0),
    })).filter(row => stripAnsi(row).trim() === '│  │'.trim() || /^│\s+│$/.test(stripAnsi(row))).length
    expect(blankRows(60)).toBe(1)
    expect(blankRows(29)).toBe(0)
  })
})

describe('prompt overflow guards', () => {
  test('truncates a ghost hint that is wider than the terminal', () => {
    const hint = '  [help  resume  new  model  plan  harden  skill  copy  clip  share  compact  clear]'
    for (const columns of [12, 40, 80]) {
      const input = defaultInput({ columns, lines: ['/'], cursorCol: 1, placeholder: false, ghostHint: hint })
      for (const row of renderLines(input)) expect(visibleWidth(row)).toBeLessThanOrEqual(columns)
    }
    // The hint still renders when there is room for part of it.
    expect(renderPlain(defaultInput({ columns: 40, lines: ['/'], cursorCol: 1, placeholder: false, ghostHint: hint })))
      .toContain('[help')
  })

  test('truncates the placeholder on a narrow terminal', () => {
    for (const columns of [3, 4, 20]) {
      const input = defaultInput({ columns })
      for (const row of renderLines(input)) expect(visibleWidth(row)).toBeLessThanOrEqual(columns)
    }
  })

  test('drops the commands half of the placeholder below 65 columns', () => {
    const hintRow = (columns: number) => renderPlain(defaultInput({ columns }))
      .split('\n').find(row => row.includes('Enter a coding task'))
    expect(hintRow(65)).toContain('Enter a coding task or / for commands')
    expect(hintRow(64)).toContain('Enter a coding task')
    expect(hintRow(64)).not.toContain('/ for commands')
    // The short hint fits outright, so it is never cut mid-word.
    expect(hintRow(45)).not.toContain('…')
  })

  test('truncates the exit hint on a narrow terminal', () => {
    for (const row of renderLines(defaultInput({ columns: 10, exitHint: true }))) {
      expect(visibleWidth(row)).toBeLessThanOrEqual(10)
    }
  })

  test('measures the zero-width cursor marker as zero columns', () => {
    // `stringWidth` counts the APC marker as five columns, which would make
    // every padded row four cells short.
    expect(visibleWidth(`❯ hi${CURSOR_MARKER}x`)).toBe(5)
  })
})

describe('prompt footer', () => {
  test('renders repository state and model identity', () => {
    const plain = renderPlain(defaultInput({
      planning: true,
      logMode: true,
      provider: 'anthropic',
      thinkingLevel: 'xhigh',
      columns: 160,
    }))
    // The border names the modes, so the footer drops its own prefix rather
    // than repeating the same words two rows apart.
    expect(plain).toContain('╭─ log · plan ')
    expect(plain).not.toContain('[log]')
    expect(plain).not.toContain('[plan]')
    expect(plain).toContain('/Users/test/project (main)')
    expect(plain).toContain('claude-sonnet@anthropic • xhigh')
  })

  test('footer keeps the mode prefix when no border carries it', () => {
    // Below the border threshold nothing above the footer names the mode, so
    // the footer stays authoritative.
    const plain = renderPlain(defaultInput({ planning: true, logMode: true, rows: 9, columns: 160 }))
    expect(plain).not.toContain('╭')
    expect(plain).toContain('[log] [plan]')
  })

  test('footer keeps the mode prefix when asked directly, as overlays do', () => {
    // Selector and ask overlays replace the composer but keep the footer, so
    // the default must carry the mode.
    const footer = blocksToLines(buildPromptFooterBlocks(defaultInput({ planning: true, columns: 160 })))
      .map(stripAnsi)[0]!
    expect(footer).toContain('[plan]')
  })

  test('labels disabled thinking', () => {
    expect(renderPlain(defaultInput({ thinkingLevel: 'off' }))).toContain('thinking off')
  })

  test('renders context and dashboard when space allows', () => {
    const plain = renderPlain(defaultInput({
      columns: 220,
      contextTokens: 105800,
      contextWindow: 272000,
      dashboardUrl: 'http://127.0.0.1:8788',
    }))
    expect(plain).toContain('context: 38.9% (105.8k/272k)')
    expect(plain).toContain('http://127.0.0.1:8788')
    // Session token totals are call/log data, not footer state.
    expect(plain).not.toContain('↑')
    expect(plain).not.toContain('cache')
  })

  test('right-aligns the dashboard link', () => {
    const columns = 120
    const footer = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      dashboardUrl: 'http://127.0.0.1:8082',
    }))).map(stripAnsi)[0]!

    expect(stringWidth(footer)).toBe(columns)
    expect(footer).toEndWith('dashboard http://127.0.0.1:8082')
  })

  test('matches the full context footer format from the terminal', () => {
    const home = process.env.HOME || process.env.USERPROFILE || '/tmp/home'
    const footer = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns: 160,
      cwd: `${home}/github/evotai/evot`,
      gitBranch: 'main',
      model: 'gpt-5.6-sol',
      provider: 'anthropic',
      thinkingLevel: 'high',
      contextTokens: 105800,
      contextWindow: 272000,
    }))).map(stripAnsi)[0]!

    expect(footer).toBe('~/github/evotai/evot (main) │ gpt-5.6-sol@anthropic • high │ context: 38.9% (105.8k/272k)')
  })

  test('carries no cache segment: per-call cache usage belongs to the spinner', () => {
    const footerAt = (columns: number) => blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      model: 'gpt-5.6-sol',
      provider: 'anthropic',
      thinkingLevel: 'high',
      contextTokens: 105800,
      contextWindow: 272000,
    }))).map(stripAnsi)[0]!

    for (const columns of [200, 160, 80]) {
      expect(footerAt(columns)).not.toContain('cache')
    }
    const wide = footerAt(160)
    expect(wide).toContain('context: 38.9% (105.8k/272k)')
    expect(wide).toContain('@anthropic')
  })

  test('degrades footer details in priority order as width narrows', () => {
    const footerAt = (columns: number) => blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      model: 'gpt-5.6-sol',
      provider: 'anthropic',
      thinkingLevel: 'max',
      dashboardUrl: 'http://127.0.0.1:8082',
      contextTokens: 105800,
      contextWindow: 272000,
    }))).map(stripAnsi)[0]!

    const withoutDashboard = footerAt(119)
    expect(withoutDashboard).not.toContain('dashboard')
    expect(withoutDashboard).toContain('context: 38.9% (105.8k/272k)')

    const compactContext = footerAt(80)
    expect(compactContext).toContain('gpt-5.6-sol@anthropic • max')
    expect(compactContext).toContain('context: 38.9%')
    expect(compactContext).not.toContain('105.8k')

    const withoutProvider = footerAt(70)
    expect(withoutProvider).toContain('gpt-5.6-sol • max')
    expect(withoutProvider).not.toContain('@anthropic')
    expect(withoutProvider).toContain('(main)')

    const withoutBranch = footerAt(60)
    expect(withoutBranch).not.toContain('(main)')
    expect(withoutBranch).toContain('context: 38.9%')

    const withoutContext = footerAt(50)
    expect(withoutContext).toContain('gpt-5.6-sol • max')
    expect(withoutContext).not.toContain('context:')

    for (const columns of [119, 80, 70, 60, 50, 30, 20]) {
      expect(stringWidth(footerAt(columns))).toBeLessThanOrEqual(columns)
    }
  })

  test('truncates a wide CJK cwd only after optional segments are gone', () => {
    const columns = 24
    const footer = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      cwd: '/项目/非常长的中文目录名称/子目录',
      gitBranch: 'feature/very-long-branch',
      model: 'a-very-long-model-name',
      provider: 'provider',
    }))).map(stripAnsi)[0]!
    expect(stringWidth(footer)).toBeLessThanOrEqual(columns)
    expect(footer).toStartWith('…')
  })

  test('footer chip advertises ↓ while the composer is empty', () => {
    const lines = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns: 100,
      backgroundProcessCount: 2,
      backgroundPanelDownAvailable: true,
    }))).map(stripAnsi)
    expect(lines[0]).toBe('2 shells running · ↓ to manage')
    expect(lines[1]).toContain('/Users/test/project')
    expect(lines[2]).toBe('')
  })

  test('with text in the composer the chip names Ctrl+T instead', () => {
    // ↓ still moves the caret there, so advertising it would be a lie.
    const lines = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns: 100,
      backgroundProcessCount: 2,
      backgroundPanelDownAvailable: false,
    }))).map(stripAnsi)
    expect(lines[0]).toBe('2 shells running · ctrl+t to manage')
  })

  test('a single shell reads in the singular', () => {
    const lines = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns: 100,
      backgroundProcessCount: 1,
      backgroundPanelDownAvailable: true,
    }))).map(stripAnsi)
    expect(lines[0]).toBe('1 shell running · ↓ to manage')
  })

  test('no chip is rendered when nothing runs in the background', () => {
    const lines = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns: 100,
      backgroundProcessCount: 0,
    }))).map(stripAnsi)
    expect(lines[0]).toContain('/Users/test/project')
  })

  test('a narrow terminal drops the hint before the count', () => {
    // The count is the actionable part, so the gesture is what gives way.
    const lines = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns: 20,
      backgroundProcessCount: 3,
      backgroundPanelDownAvailable: true,
    }))).map(stripAnsi)
    expect(lines[0]).toBe('3 shells running')
    expect(stringWidth(lines[0]!)).toBeLessThanOrEqual(20)
  })

  test('footer remains available without the editor', () => {
    const lines = blocksToLines(buildPromptFooterBlocks(defaultInput({ provider: 'openai', model: 'gpt-5.6-sol' }))).map(stripAnsi)
    expect(lines).toHaveLength(2)
    expect(lines[0]).toContain('gpt-5.6-sol@openai')
    expect(lines[1]).toBe('')
    expect(lines.join('\n')).not.toContain('Enter a coding task or / for commands')
  })
})

describe('joinLeftRight', () => {
  test('right-aligns the trailing spans on one row', () => {
    const left = [{ text: '* Waiting for model… (7.5s) · esc to interrupt' }]
    const right = [{ text: '⬇ Auto-updating to v2026.8.26.2…' }]
    const columns = 120
    const joined = joinLeftRight(left, right, columns)
    expect(spansWidth(joined)).toBe(columns)
    expect(joined.map(span => span.text).join('')).toEndWith('⬇ Auto-updating to v2026.8.26.2…')
    expect(joined[1]!.text.startsWith(' ')).toBe(true)
  })

  test('keeps a one-column gap when the two sides collide', () => {
    const joined = joinLeftRight([{ text: 'left' }], [{ text: 'right' }], 8)
    expect(joined.map(span => span.text).join('')).toBe('left right')
  })
})
