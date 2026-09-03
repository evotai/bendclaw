import { beforeAll, describe, expect, test } from 'bun:test'
import chalk from 'chalk'
import stripAnsi from 'strip-ansi'

beforeAll(() => {
  // These renderers exist to emit colour, so the suite has to run at a level
  // where chalk actually does. Bun's runner has no TTY, where chalk.level is 0
  // and every hue assertion would pass against empty output.
  chalk.level = 3
})

import {
  gridLines,
  renderNotice,
  renderOperation,
  renderProgress,
  renderRemoved,
  renderSkillList,
  renderSkillSummary,
  shortMemberName,
  skillSummaryParts,
  type SkillListView,
  type UnitResult,
} from '../src/commands/skill/render.js'

function lines(text: string): string[] {
  return stripAnsi(text).split('\n')
}

function unit(overrides: Partial<UnitResult> = {}): UnitResult {
  return { name: 'lark', skills: 27, outcome: 'new', detail: '', notes: [], ...overrides }
}

function view(overrides: Partial<SkillListView> = {}): SkillListView {
  return {
    total: 3,
    units: [
      { name: 'lark', label: 'lark/', origin: '@40b5130', members: ['lark-im', 'lark-doc'] },
      { name: 'zero-tech-debt', label: 'zero-tech-debt', origin: '~/.evotai/skills', members: [] },
    ],
    ...overrides,
  }
}

describe('gridLines', () => {
  test('packs as many columns as the width allows', () => {
    const items = ['aa', 'bb', 'cc', 'dd']
    // indent 4 + 4 items of width 2 + 3 gaps of 3 = 21 columns.
    expect(gridLines(items, 40)).toEqual(['    aa   bb   cc   dd'])
  })

  test('falls back to one item per row when nothing else fits', () => {
    expect(gridLines(['alpha', 'beta'], 10)).toEqual(['    alpha', '    beta'])
  })

  test('fills top to bottom so a long name widens only its own column', () => {
    // Two columns: `a`/`bbbbbb` then `ccc`/`d`. `bbbbbb` sets column 0's width
    // and nothing in column 1 pays for it.
    expect(gridLines(['a', 'bbbbbb', 'ccc', 'd'], 19)).toEqual([
      '    a        ccc',
      '    bbbbbb   d',
    ])
  })

  test('a short last row pads no trailing cells', () => {
    for (const row of gridLines(['aa', 'bb', 'ccc'], 30)) {
      expect(row).toBe(row.trimEnd())
    }
  })

  test('never exceeds the given width', () => {
    const items = Array.from({ length: 30 }, (_, i) => `skill-name-${i}`)
    for (const width of [40, 60, 80, 120]) {
      for (const row of gridLines(items, width)) {
        expect(row.length).toBeLessThanOrEqual(width)
      }
    }
  })

  test('an empty set produces no rows', () => {
    expect(gridLines([], 80)).toEqual([])
  })
})

describe('shortMemberName', () => {
  test('drops the group prefix the group row already shows', () => {
    expect(shortMemberName('lark', 'lark-im')).toBe('im')
    expect(shortMemberName('lark', 'lark-workflow-standup-report')).toBe('workflow-standup-report')
  })

  test('leaves names that are not prefixed, or are only the prefix', () => {
    expect(shortMemberName('lark', 'databend-cloud')).toBe('databend-cloud')
    expect(shortMemberName('lark', 'lark')).toBe('lark')
    expect(shortMemberName('lark', 'lark-')).toBe('lark-')
  })
})

describe('renderSkillList', () => {
  test('heads with the skill and unit counts', () => {
    expect(lines(renderSkillList(view(), 80))[1]).toBe('  Skills  3 · 2 units')
  })

  test('singular unit count reads naturally', () => {
    const single = view({
      total: 1,
      units: [{ name: 'solo', label: 'solo', origin: 'builtin', members: [] }],
    })
    expect(lines(renderSkillList(single, 80))[1]).toBe('  Skills  1 · 1 unit')
  })

  test('groups carry a filled marker, a count, and their members below', () => {
    const out = lines(renderSkillList(view(), 80))
    expect(out).toContain('  ● lark/           @40b5130  2')
    expect(out).toContain('    im   doc')
  })

  test('the count column is sized from group origins, not long directory paths', () => {
    const long = view({
      units: [
        { name: 'lark', label: 'lark/', origin: '@40b5130', members: ['lark-im'] },
        { name: 'local', label: 'local', origin: '~/very/long/path/to/skills/dir', members: [] },
      ],
    })
    // The count follows its own origin by one gap; the lone skill's much wider
    // path does not push it right.
    expect(lines(renderSkillList(long, 80))).toContain('  ● lark/  @40b5130  1')
  })

  test('lone skills carry a hollow marker and no count column', () => {
    expect(lines(renderSkillList(view(), 80))).toContain('  ○ zero-tech-debt  ~/.evotai/skills')
  })

  test('label and origin columns are shared across groups and lone skills', () => {
    const out = lines(renderSkillList(view(), 80))
    const group = out.find((row) => row.includes('lark/'))!
    const lone = out.find((row) => row.includes('zero-tech-debt'))!
    expect(group.indexOf('@40b5130')).toBe(lone.indexOf('~/.evotai/skills'))
  })

  test('closes with the subcommand hint', () => {
    expect(lines(renderSkillList(view(), 80)).at(-1)).toBe(
      '  /skill install <source> · update [name] · remove <name>',
    )
  })

  test('an empty view says so in one line, styled like every other notice', () => {
    const out = renderSkillList({ units: [], total: 0 }, 80)
    expect(stripAnsi(out)).toBe('  no skills installed')
    expect(out).toBe(renderNotice('no skills installed'))
  })

  test('every row fits the terminal width', () => {
    const members = Array.from({ length: 27 }, (_, i) => `lark-member-${i}`)
    const wide = view({ total: 28, units: [{ name: 'lark', label: 'lark/', origin: '@40b5130', members }] })
    for (const row of lines(renderSkillList(wide, 60))) {
      expect(row.length).toBeLessThanOrEqual(60)
    }
  })
})

describe('renderSkillSummary', () => {
  test('collapses group members into a count', () => {
    // The banner is a glance: 27 spelled-out lark names cost three wrapped
    // lines and named nothing the user could act on.
    expect(stripAnsi(renderSkillSummary(view()))).toBe('lark/ 2 · zero-tech-debt')
  })

  test('labels match the ones /skill list shows', () => {
    const parts = skillSummaryParts(view()).map(stripAnsi)
    const listed = lines(renderSkillList(view(), 80))
    for (const part of parts) {
      const label = part.split(' ')[0]!
      expect(listed.some((row) => row.includes(label))).toBe(true)
    }
  })

  test('a lone skill carries no count', () => {
    const single = view({
      total: 1,
      units: [{ name: 'solo', label: 'solo', origin: 'builtin', members: [] }],
    })
    expect(stripAnsi(renderSkillSummary(single))).toBe('solo')
  })

  test('an empty view produces nothing to render', () => {
    expect(skillSummaryParts({ units: [], total: 0 })).toEqual([])
    expect(renderSkillSummary({ units: [], total: 0 })).toBe('')
  })

  test('an explicit muted hue paints the counts and separators', () => {
    // The banner's secondary gray differs from the REPL's; left to the default
    // the counts would sit a shade off from the [Context] values above them.
    const out = renderSkillSummary(view(), '#808080')
    expect(out).toContain('\x1b[38;2;128;128;128m')
    expect(out).not.toContain('\x1b[38;2;119;119;119m')
    expect(stripAnsi(out)).toBe('lark/ 2 · zero-tech-debt')
  })
})

describe('renderOperation', () => {
  test('install names the resolved source and the closing count', () => {
    const out = lines(renderOperation({
      title: 'Installed',
      source: 'evotai/evot-skills@40b5130',
      units: [unit()],
      total: 28,
    }))
    expect(out[0]).toBe('  Installed  evotai/evot-skills@40b5130')
    expect(out).toContain('  ✓ lark/  27 skills')
    expect(out.at(-1)).toBe('  28 skills installed')
  })

  test('a lone skill shows no size column', () => {
    const out = lines(renderOperation({
      title: 'Installed',
      units: [unit({ name: 'databend-cloud', skills: 1 })],
    }))
    expect(out).toContain('  ✓ databend-cloud')
  })

  test('each outcome gets its own marker', () => {
    const out = lines(renderOperation({
      title: 'Updated',
      units: [
        unit({ name: 'lark', outcome: 'updated', detail: '3f9c2a1 → 40b5130' }),
        unit({ name: 'databend-cloud', skills: 1, outcome: 'unchanged', detail: '40b5130' }),
        unit({ name: 'zero-tech-debt', skills: 1, outcome: 'skipped', detail: 'local' }),
        unit({ name: 'broken', skills: 1, outcome: 'failed', detail: 'network down' }),
      ],
    }))
    expect(out).toContain('  ↑ lark/           27 skills  3f9c2a1 → 40b5130')
    expect(out).toContain('  = databend-cloud             40b5130')
    expect(out).toContain('  - zero-tech-debt             local')
    expect(out).toContain('  ✗ broken                     network down')
  })

  test('warn notes hang under their unit with an actionable glyph', () => {
    const out = lines(renderOperation({
      title: 'Installed',
      units: [unit({ notes: [{ kind: 'warn', text: 'needs /env set LARK_APP_ID=<value>' }] })],
    }))
    expect(out).toContain('    ! needs /env set LARK_APP_ID=<value>')
  })

  test('info notes report what we did, not what the user must do', () => {
    const out = lines(renderOperation({
      title: 'Installed',
      units: [unit({ notes: [{ kind: 'info', text: 'replaced standalone lark-im' }] })],
    }))
    expect(out).toContain('    · replaced standalone lark-im')
  })

  test('a report with no units is just its heading', () => {
    expect(lines(renderOperation({ title: 'Updated', units: [] }))).toEqual(['  Updated'])
  })
})

describe('single-line renderers', () => {
  test('notices and removals are indented to match every other row', () => {
    expect(stripAnsi(renderNotice('skill not found: nope'))).toBe('  skill not found: nope')
    expect(stripAnsi(renderRemoved('removed skill: lark'))).toBe('  ✓ removed skill: lark')
    expect(stripAnsi(renderProgress('extracting archive...'))).toBe('  ⋯ skill  extracting archive...')
  })
})
