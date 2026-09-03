import { describe, expect, test } from 'bun:test'
import { mkdtempSync, writeFileSync } from 'fs'
import { join } from 'path'
import { tmpdir } from 'os'

import { runEnvCommand, USAGE, type EnvPort } from '../src/commands/env/manage.js'
import { describe as describeValue, shortDate } from '../src/commands/env/format.js'
import { isValidKey, parseEnvFile } from '../src/commands/env/parse.js'
import { renderGet, renderList } from '../src/commands/env/render.js'

interface Recorded {
  sets: Array<{ key: string; value: string }>
  dels: string[]
}

function port(
  initial: Array<{ key: string; value: string; updated_at?: string }> = [],
  options: { files?: Record<string, string>; failSet?: boolean } = {},
): { port: EnvPort; recorded: Recorded } {
  const rows = [...initial]
  const recorded: Recorded = { sets: [], dels: [] }
  return {
    recorded,
    port: {
      list: () => rows,
      set: async (key, value) => {
        if (options.failSet) throw new Error('disk full')
        recorded.sets.push({ key, value })
        const existing = rows.find((row) => row.key === key)
        if (existing) existing.value = value
        else rows.push({ key, value, updated_at: '2026-09-03T00:00:00Z' })
      },
      del: async (key) => {
        recorded.dels.push(key)
        const index = rows.findIndex((row) => row.key === key)
        if (index === -1) return false
        rows.splice(index, 1)
        return true
      },
      readFile: async (path) => {
        const file = options.files?.[path]
        if (file === undefined) throw new Error('ENOENT: no such file')
        return file
      },
    },
  }
}

describe('describe', () => {
  test('reports length without revealing any character', () => {
    const secret = '855ffdca2b3d5bdf79535dc047e47fbc'
    const shown = describeValue(secret)
    expect(shown).toBe('32 chars')
    expect(shown).not.toContain('7fbc')
    expect(shown).not.toContain('855f')
  })

  test('never leaks a URL-shaped tail, which is a warehouse name not entropy', () => {
    const a = describeValue('bendcloud://o:tokenone@api.databend.com/default')
    const b = describeValue('bendcloud://p:tokentwo@api.databend.com/default')
    expect(a).not.toContain('default')
    expect(b).not.toContain('default')
    // Same length values are indistinguishable by design; the key tells them apart.
    expect(describeValue('aaaa'.repeat(4))).toBe(describeValue('bbbb'.repeat(4)))
  })

  test('empty is labelled, not measured', () => {
    expect(describeValue('')).toBe('(empty)')
  })

  test('length answers "did my paste get truncated"', () => {
    expect(describeValue('abc')).toBe('3 chars')
    expect(describeValue('a'.repeat(120))).toBe('120 chars')
  })
})

describe('shortDate', () => {
  test('renders an ISO timestamp as a date', () => {
    expect(shortDate('2026-09-03T02:11:34.598Z')).toBe('2026-09-03')
  })

  test('missing or malformed values fall back to a dash', () => {
    expect(shortDate(undefined)).toBe('-')
    expect(shortDate('not a date')).toBe('-')
  })
})

describe('parseEnvFile', () => {
  test('reads plain assignments and keeps the last duplicate', () => {
    const { entries, skipped } = parseEnvFile('A=1\nB=2\nA=3\n')
    expect(entries).toEqual([
      { key: 'A', value: '3' },
      { key: 'B', value: '2' },
    ])
    expect(skipped).toBe(0)
  })

  test('ignores comments and blank lines', () => {
    expect(parseEnvFile('# note\n\n  \nA=1\n').entries).toEqual([{ key: 'A', value: '1' }])
  })

  test('tolerates a leading export and surrounding quotes', () => {
    const { entries } = parseEnvFile('export A="1"\nexport B=\'two\'\n')
    expect(entries).toEqual([
      { key: 'A', value: '1' },
      { key: 'B', value: 'two' },
    ])
  })

  test('keeps values containing = and : intact', () => {
    expect(parseEnvFile('DSN=bendcloud://o:t@h/w?x=1\n').entries).toEqual([
      { key: 'DSN', value: 'bendcloud://o:t@h/w?x=1' },
    ])
  })

  test('counts malformed lines and invalid keys as skipped', () => {
    const { entries, skipped } = parseEnvFile('novalue\n=orphan\n1BAD=x\nA B=y\nGOOD=1\n')
    expect(entries).toEqual([{ key: 'GOOD', value: '1' }])
    expect(skipped).toBe(4)
  })
})

describe('isValidKey', () => {
  test('accepts shell-style names', () => {
    for (const key of ['A', 'BENDCLOUD_DSN', '_x', 'A1']) expect(isValidKey(key)).toBe(true)
  })

  test('rejects names bash could not export', () => {
    for (const key of ['1A', 'A-B', 'A B', '', 'A=B']) expect(isValidKey(key)).toBe(false)
  })
})

describe('renderList', () => {
  test('masks every value and shows when it changed', () => {
    const out = renderList([
      { key: 'BENDCLOUD_DSN', value: 'bendcloud://o:secrettoken1@h/w', updated_at: '2026-09-03T00:00:00Z' },
      { key: 'A', value: 'shortish', updated_at: '2026-09-01T00:00:00Z' },
    ])
    expect(out).toContain('Variables (2)')
    expect(out).not.toContain('secrettoken1')
    expect(out).toContain('2026-09-03')
    expect(out.indexOf('  A ')).toBeLessThan(out.indexOf('BENDCLOUD_DSN'))
    expect(out).toContain('--reveal')
  })

  test('empty state says so without the reveal hint', () => {
    expect(renderList([])).toBe('  no variables set')
  })
})

describe('renderGet', () => {
  test('masks by default and reveals only when asked', () => {
    const row = { key: 'K', value: 'supersecretvalue', updated_at: '2026-09-03T00:00:00Z' }
    expect(renderGet(row, 'K', false)).not.toContain('supersecretvalue')
    expect(renderGet(row, 'K', true)).toBe('  K=supersecretvalue')
  })

  test('unknown key is reported plainly', () => {
    expect(renderGet(undefined, 'NOPE', true)).toBe('  not set: NOPE')
  })
})

describe('runEnvCommand', () => {
  test('bare invocation and list are the same view', async () => {
    const { port: p } = port([{ key: 'A', value: 'value-one' }])
    expect(await runEnvCommand(p, '')).toBe(await runEnvCommand(p, 'list'))
  })

  test('set stores the value but never echoes it', async () => {
    const { port: p, recorded } = port()
    const out = await runEnvCommand(p, 'set BENDCLOUD_DSN=bendcloud://o:secrettoken@h/w')
    expect(recorded.sets).toEqual([
      { key: 'BENDCLOUD_DSN', value: 'bendcloud://o:secrettoken@h/w' },
    ])
    expect(out).not.toContain('secrettoken')
    expect(out).toContain('29 chars')
  })

  test('set keeps = and spaces inside the value', async () => {
    const { port: p, recorded } = port()
    await runEnvCommand(p, 'set Q=a=b c')
    expect(recorded.sets[0]!.value).toBe('a=b c')
  })

  test('set rejects keys bash could not export', async () => {
    const { port: p, recorded } = port()
    expect(await runEnvCommand(p, 'set 1BAD=x')).toContain('invalid key')
    expect(recorded.sets).toEqual([])
  })

  test('set without = explains itself', async () => {
    const { port: p } = port()
    expect(await runEnvCommand(p, 'set JUSTKEY')).toContain('Usage: /env set')
  })

  test('a failing write propagates instead of reporting success', async () => {
    const { port: p } = port([], { failSet: true })
    await expect(runEnvCommand(p, 'set A=1')).rejects.toThrow('disk full')
  })

  test('del distinguishes a real deletion from a missing key', async () => {
    const { port: p, recorded } = port([{ key: 'A', value: 'x' }])
    expect(await runEnvCommand(p, 'del A')).toBe('  deleted A')
    expect(await runEnvCommand(p, 'del A')).toBe('  not set: A')
    expect(recorded.dels).toEqual(['A', 'A'])
  })

  test('get honours --reveal in either position', async () => {
    const { port: p } = port([{ key: 'K', value: 'plainsecret' }])
    expect(await runEnvCommand(p, 'get K')).not.toContain('plainsecret')
    expect(await runEnvCommand(p, 'get K --reveal')).toContain('plainsecret')
    expect(await runEnvCommand(p, 'get --reveal K')).toContain('plainsecret')
  })

  test('unknown subcommand shows the full usage', async () => {
    const { port: p } = port()
    expect(await runEnvCommand(p, 'bogus')).toBe(USAGE)
    expect(USAGE).toContain('list')
    expect(USAGE).toContain('load FILE')
  })

  test('load imports each entry and reports names only', async () => {
    const { port: p, recorded } = port([], {
      files: { '/tmp/x.env': 'A=secretone\nB=secrettwo\n# c\nbroken\n' },
    })
    const out = await runEnvCommand(p, 'load /tmp/x.env')
    expect(recorded.sets).toEqual([
      { key: 'A', value: 'secretone' },
      { key: 'B', value: 'secrettwo' },
    ])
    expect(out).toContain('loaded 2 variable(s)')
    expect(out).toContain('1 line(s) skipped')
    expect(out).toContain('A, B')
    expect(out).not.toContain('secretone')
  })

  test('load reports an unreadable file without throwing', async () => {
    const { port: p } = port()
    expect(await runEnvCommand(p, 'load /nope.env')).toContain('cannot read /nope.env')
  })

  test('load says so when a readable file has nothing usable', async () => {
    const { port: p } = port([], { files: { '/tmp/e.env': '# only comments\n' } })
    expect(await runEnvCommand(p, 'load /tmp/e.env')).toContain('no KEY=VALUE lines')
  })

  test('load reads a real file from disk', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'evot-env-'))
    const file = join(dir, 'creds.env')
    writeFileSync(file, 'BENDCLOUD_DSN=bendcloud://o:t@h/w\n')
    const { port: p, recorded } = port([], {})
    const real: EnvPort = {
      ...p,
      readFile: async (path) => (await import('fs/promises')).readFile(path, 'utf8'),
    }
    expect(await runEnvCommand(real, `load ${file}`)).toContain('loaded 1 variable(s)')
    expect(recorded.sets[0]!.key).toBe('BENDCLOUD_DSN')
  })

  test('subcommands each explain themselves when given no argument', async () => {
    const { port: p } = port()
    expect(await runEnvCommand(p, 'get')).toContain('Usage: /env get')
    expect(await runEnvCommand(p, 'del')).toContain('Usage: /env del')
    expect(await runEnvCommand(p, 'load')).toContain('Usage: /env load')
  })
})
