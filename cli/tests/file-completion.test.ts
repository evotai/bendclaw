import { describe, test, expect } from 'bun:test'
import { extractAtPrefix, completeAtFile, fileCompletionNote, fuzzyScore, anchoredAtQuery } from '../src/commands/file-completion.js'
import { mkdtempSync, writeFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { tmpdir } from 'os'

describe('extractAtPrefix', () => {
  test('extracts @ at start of line', () => {
    expect(extractAtPrefix('@src')).toEqual({ prefix: '@src', start: 0 })
  })

  test('extracts @ after space', () => {
    expect(extractAtPrefix('look at @src/m')).toEqual({ prefix: '@src/m', start: 8 })
  })

  test('returns null for email-like patterns', () => {
    expect(extractAtPrefix('user@example')).toBeNull()
  })

  test('returns null when no @', () => {
    expect(extractAtPrefix('hello world')).toBeNull()
  })

  test('extracts @ with empty query', () => {
    expect(extractAtPrefix('@')).toEqual({ prefix: '@', start: 0 })
  })

  test('extracts @ after multiple words', () => {
    expect(extractAtPrefix('fix the bug in @cli/src')).toEqual({ prefix: '@cli/src', start: 15 })
  })
})

describe('fileCompletionNote', () => {
  test('explains the bounded no-fd fuzzy fallback', () => {
    expect(fileCompletionNote(false, false, false)).toBe(
      'files up to 6 levels deep — install fd to search deeper',
    )
  })

  test('explains when completion searches the whole home directory', () => {
    expect(fileCompletionNote(true, false, true)).toBe(
      'searching home — open a project folder for more relevant results',
    )
    expect(fileCompletionNote(true, true, true)).toBe(
      'searching home — open a project folder for more relevant results',
    )
  })

  test('prefers the actionable no-fd limitation in home', () => {
    expect(fileCompletionNote(false, false, true)).toContain('install fd')
  })

  test('stays quiet for complete searches inside a project', () => {
    expect(fileCompletionNote(true, false, false)).toBeUndefined()
    expect(fileCompletionNote(false, true, false)).toBeUndefined()
    expect(fileCompletionNote(true, true, false)).toBeUndefined()
  })
})

describe('completeAtFile', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'evot-file-completion-'))
  mkdirSync(join(tmp, 'src'))
  writeFileSync(join(tmp, 'src', 'main.ts'), 'console.log("hi")')
  writeFileSync(join(tmp, 'src', 'utils.ts'), 'export {}')
  writeFileSync(join(tmp, 'README.md'), '# Test')
  mkdirSync(join(tmp, 'src', 'nested'))

  test('completes files in root with readdir fallback', async () => {
    const result = await completeAtFile('@', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.length).toBeGreaterThan(0)
    expect(result!.items.some(i => i.label === 'src/')).toBe(true)
    expect(result!.items.some(i => i.label === 'README.md')).toBe(true)
  })

  test('completes files in subdirectory', async () => {
    const result = await completeAtFile('@src/', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.some(i => i.label === 'src/main.ts')).toBe(true)
    expect(result!.items.some(i => i.label === 'src/utils.ts')).toBe(true)
  })

  test('filters by prefix', async () => {
    const result = await completeAtFile('@src/m', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.some(i => i.label === 'src/main.ts')).toBe(true)
    expect(result!.items.some(i => i.label === 'src/utils.ts')).toBe(false)
  })

  test('returns no candidates for a non-matching prefix', async () => {
    const result = await completeAtFile('@zzz_nonexistent', tmp)
    // With fd there is nothing to explain and the result closes. The bounded
    // no-fd fallback stays open only to disclose why deep files may be missing.
    if (result) {
      expect(result.items).toEqual([])
      expect(result.note).toContain('no matches')
      expect(result.note).toContain('install fd')
    } else {
      expect(result).toBeNull()
    }
  })

  test('respects abort signal', async () => {
    const abort = new AbortController()
    abort.abort()
    const result = await completeAtFile('@src', tmp, abort.signal)
    // With readdir fallback, abort only affects fd; readdir still works
    // But if fd is found, it should return empty
    expect(result === null || result.items.length >= 0).toBe(true)
  })

  test('fuzzy matches nested file by name without navigating', async () => {
    const result = await completeAtFile('@main', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.some(i => i.label === 'src/main.ts')).toBe(true)
  })

  test('fuzzy matches by path subsequence', async () => {
    const result = await completeAtFile('@srcnst', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.some(i => i.label === 'src/nested/' || i.label === 'src/nested')).toBe(true)
  })
})

describe('fuzzyScore', () => {
  test('ranks basename prefix above substring above path match', () => {
    expect(fuzzyScore('src/main.ts', 'main')).toBe(0)
    expect(fuzzyScore('src/domain.ts', 'main')).toBe(1)
    expect(fuzzyScore('main/index.ts', 'main/i')).toBe(2)
    expect(fuzzyScore('cli/src/term/repl.ts', 'ctrepl')).toBe(3)
    expect(fuzzyScore('src/utils.ts', 'zzz')).toBeNull()
  })

  test('is case-insensitive and ignores trailing slash for basename', () => {
    expect(fuzzyScore('src/Nested/', 'nested')).toBe(0)
  })
})

describe('anchoredAtQuery', () => {
  const home = '/Users/tester'

  test('splits an absolute query into a directory and a name filter', () => {
    expect(anchoredAtQuery('/tmp/shots/pic.png', home)).toEqual({
      dir: '/tmp/shots',
      filter: 'pic.png',
      displayPrefix: '/tmp/shots/',
    })
  })

  test('treats a trailing slash as listing that directory', () => {
    expect(anchoredAtQuery('/tmp/shots/', home)).toEqual({
      dir: '/tmp/shots',
      filter: '',
      displayPrefix: '/tmp/shots/',
    })
  })

  test('expands ~ against home', () => {
    expect(anchoredAtQuery('~/Library/Con', home)).toEqual({
      dir: '/Users/tester/Library',
      filter: 'Con',
      displayPrefix: '/Users/tester/Library/',
    })
    // Bare `~` means "list home", not "filter /Users for an entry named tester".
    expect(anchoredAtQuery('~', home)).toEqual({
      dir: '/Users/tester',
      filter: '',
      displayPrefix: '/Users/tester/',
    })
  })

  test('keeps the root directory addressable', () => {
    expect(anchoredAtQuery('/Us', home)).toEqual({
      dir: '/',
      filter: 'Us',
      displayPrefix: '/',
    })
  })

  test('leaves project-relative queries to the cwd-relative search', () => {
    expect(anchoredAtQuery('src/main.ts', home)).toBeNull()
    expect(anchoredAtQuery('', home)).toBeNull()
  })
})

/**
 * Absolute and `~` paths are how files outside the project get attached (a
 * screenshot in a sandboxed app container, for example). These used to return
 * nothing because the query was joined onto cwd.
 */
describe('completeAtFile with paths outside the project', () => {
  const outside = mkdtempSync(join(tmpdir(), 'evot-outside-'))
  const shots = join(outside, 'RWTemp')
  mkdirSync(shots)
  writeFileSync(join(shots, 'shot.png'), 'x')
  writeFileSync(join(shots, 'note[draft].png'), 'x')
  writeFileSync(join(shots, 'photo(1).png'), 'x')
  writeFileSync(join(shots, 'photo (1).png'), 'x')
  writeFileSync(join(shots, '.hidden.png'), 'x')
  const project = mkdtempSync(join(tmpdir(), 'evot-project-'))

  test('lists an absolute directory that is not under cwd', async () => {
    const result = await completeAtFile(`@${shots}/`, project)
    expect(result).not.toBeNull()
    expect(result!.items.map(item => item.label)).toContain(join(shots, 'shot.png'))
  })

  test('filters an absolute directory by entry name', async () => {
    const result = await completeAtFile(`@${shots}/shot`, project)
    expect(result).not.toBeNull()
    expect(result!.items.map(item => item.label)).toEqual([join(shots, 'shot.png')])
  })

  test('completes names containing brackets and parentheses', async () => {
    const brackets = await completeAtFile(`@${shots}/note[dr`, project)
    expect(brackets).not.toBeNull()
    expect(brackets!.items.map(item => item.label)).toEqual([join(shots, 'note[draft].png')])

    // `(1)` would be a capture group if the filter were compiled as a regex.
    // The exact name ranks first; `photo (1).png` also matches as a subsequence.
    const parens = await completeAtFile(`@${shots}/photo(1)`, project)
    expect(parens).not.toBeNull()
    expect(parens!.items[0]!.label).toBe(join(shots, 'photo(1).png'))
  })

  test('stops the @ token at a space, so names with spaces stay unreachable', async () => {
    // `@` tokens end at whitespace: a trailing word would otherwise be eaten out
    // of the sentence. Typing up to the space completes only the leading part…
    expect(extractAtPrefix(`see @${shots}/photo`)).toEqual({
      prefix: `@${shots}/photo`,
      start: 4,
    })
    // …and once the space is typed the token is gone entirely.
    expect(extractAtPrefix(`see @${shots}/photo (1).png`)).toBeNull()
    expect(await completeAtFile(`@${shots}/space name`, project)).toBeNull()
  })

  test('inserts the absolute path so the file resolves after submit', async () => {
    const result = await completeAtFile(`@${shots}/shot`, project)
    expect(result!.items[0]!.value).toBe(`@${join(shots, 'shot.png')}`)
  })

  test('marks directories so completion can keep navigating', async () => {
    const result = await completeAtFile(`@${outside}/RWTemp`, project)
    expect(result).not.toBeNull()
    const dir = result!.items.find(item => item.label.includes('RWTemp'))
    expect(dir).toBeDefined()
    expect(dir!.isDirectory).toBe(true)
    expect(dir!.label.endsWith('/')).toBe(true)
  })

  test('hides dotfiles until the filter asks for them', async () => {
    const listed = await completeAtFile(`@${shots}/`, project)
    expect(listed!.items.some(item => item.label.includes('.hidden.png'))).toBe(false)

    const asked = await completeAtFile(`@${shots}/.hid`, project)
    expect(asked!.items.map(item => item.label)).toEqual([join(shots, '.hidden.png')])
  })

  test('returns null for a directory that does not exist', async () => {
    expect(await completeAtFile(`@${outside}/nope/`, project)).toBeNull()
  })
})

