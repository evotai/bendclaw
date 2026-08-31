import { describe, test, expect } from 'bun:test'
import { complete, getGhostHint } from '../src/commands/completion.js'

describe('slash command completion', () => {
  test('completes single match with trailing space', () => {
    const result = complete('/he', 3)
    expect(result).not.toBeNull()
    expect(result!.replacement).toBe('/help ')
    expect(result!.candidates).toEqual(['/help'])
    expect(result!.wordStart).toBe(0)
  })

  test('completes common prefix for multiple matches', () => {
    // /e matches /exit and /env — should show both candidates
    const result = complete('/e', 2)
    expect(result).not.toBeNull()
    expect(result!.candidates).toContain('/exit')
    expect(result!.candidates).toContain('/env')
  })

  test('returns null for no matches', () => {
    const result = complete('/zzz', 4)
    expect(result).toBeNull()
  })

  test('completes /ha to /harden', () => {
    const result = complete('/ha', 3)
    expect(result).not.toBeNull()
    expect(result!.replacement).toBe('/harden ')
    expect(result!.candidates).toEqual(['/harden'])
  })

  test('returns null when past the command word', () => {
    const result = complete('/model gpt', 10)
    expect(result).toBeNull()
  })

  test('completes /rest to /restart', () => {
    const result = complete('/rest', 5)
    expect(result).not.toBeNull()
    expect(result!.replacement).toBe('/restart ')
    expect(result!.candidates).toEqual(['/restart'])
  })

  test('completes /q to /quit alias', () => {
    const result = complete('/qu', 3)
    expect(result).not.toBeNull()
    expect(result!.candidates).toContain('/quit')
  })

  test('shows multiple candidates for ambiguous prefix', () => {
    // /cl could match /clear
    const result = complete('/cl', 3)
    expect(result).not.toBeNull()
    expect(result!.candidates).toContain('/clear')
  })
})

describe('file path completion', () => {
  test('completes paths starting with ./', () => {
    const result = complete('./', 2)
    expect(result).not.toBeNull()
    expect(result!.candidates.length).toBeGreaterThan(0)
  })

  test('returns null for plain text', () => {
    const result = complete('hello world', 11)
    expect(result).toBeNull()
  })

  test('treats existing directory as a parent for descent (appends trailing /)', () => {
    // The cwd of the test runner is the cli/ directory, which contains a
    // `tests` subdirectory. Typing `./tests` (no slash) and pressing tab
    // should produce candidates that include a trailing slash, so a follow-up
    // tab descends into the directory rather than concatenating filenames
    // directly to the typed path.
    const result = complete('./tests', 7)
    expect(result).not.toBeNull()
    // Either a single match `./tests/` or, if multiple siblings begin with
    // `tests`, all of them must keep a clear separator before any filename.
    for (const cand of result!.candidates) {
      // No candidate should look like "./testsfoo" — the segment after
      // `./tests` must start with `/` (directory boundary) or the candidate
      // must be a sibling whose name begins with `tests`.
      if (cand.startsWith('./tests') && cand !== './tests/') {
        const after = cand.slice('./tests'.length)
        // Either it's a deeper path ("./tests/...") or a sibling name
        // ("./testsuite" — starts with extra letters then maybe `/`).
        // What we must never see is something like "./testsfile.ts" where
        // the original `./tests` was a real directory but got concatenated
        // with a child filename without a slash.
        if (after.length > 0 && after[0] !== '/' && /^[a-z]/i.test(after[0]!)) {
          // Allow only when this is genuinely a sibling, not a child of ./tests
          // For the tests/ directory in the cli workspace there is no sibling
          // entry whose name starts with "tests" other than `tests/` itself.
          throw new Error(`unexpected candidate without separator: ${cand}`)
        }
      }
    }
  })

  test('extracts path glued to CJK text without space', () => {
    // "看下./tests" — a path appended directly to CJK characters with no
    // separating whitespace. Tab should still recognise the path segment.
    const line = '看下./tests'
    const result = complete(line, line.length)
    expect(result).not.toBeNull()
    expect(result!.replacement.startsWith('./tests')).toBe(true)
    // wordStart must point at `.` of `./tests`, not the start of the line.
    expect(line.slice(result!.wordStart)).toBe('./tests')
  })

  test('extracts path mid-line preceded by ASCII text and space', () => {
    const line = 'edit ./tests rest'
    // cursor right after `./tests`
    const cursor = 'edit ./tests'.length
    const result = complete(line, cursor)
    expect(result).not.toBeNull()
    expect(line.slice(result!.wordStart, cursor)).toBe('./tests')
  })

  test('does not trigger for bare ~ without slash', () => {
    const result = complete('please look at ~', 16)
    expect(result).toBeNull()
  })
})

describe('ghost hints', () => {
  test('shows clip all subcommand', () => {
    expect(getGhostHint('/clip ', 6)).toContain('all')
    expect(getGhostHint('/clip a', 7)).toBe('ll')
  })

  test('shows skill subcommands', () => {
    const hint = getGhostHint('/skill ', 7)
    expect(hint).toContain('install')
    expect(hint).toContain('list')
    expect(hint).toContain('remove')
  })

  test('hints the /sessions alias like the canonical command', () => {
    // Typing an alias must feel like typing the real name: same description on
    // the name, same argument hints after the space.
    expect(getGhostHint('/sessions', 9)).toContain('<id>')
    expect(getGhostHint('/sessions ', 10)).toContain('<query>')
  })
})

describe('alias completion', () => {
  test('tab-completes /sessions', () => {
    const result = complete('/sess', 5)
    expect(result).not.toBeNull()
    expect(result!.replacement).toBe('/sessions ')
    expect(result!.candidates).toEqual(['/sessions'])
  })
})
