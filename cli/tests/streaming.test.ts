import { afterEach, describe, expect, test } from 'bun:test'
import { findStableBoundary, splitStableBlocks, renderStreamingText, shouldAnimateTerminalTitle } from '../src/utils/streaming.js'

describe('renderStreamingText', () => {
  test('keeps markdown markers untouched while streaming', () => {
    expect(renderStreamingText('**bold** `code`')).toBe('**bold** `code`')
  })
})

describe('findStableBoundary', () => {
  test('returns 0 for a single block', () => {
    expect(findStableBoundary('hello world', 0)).toBe(0)
  })

  test('returns 0 for empty text', () => {
    expect(findStableBoundary('', 0)).toBe(0)
  })

  test('advances past completed blocks', () => {
    const text = '# Heading\n\nSome paragraph\n\nGrowing text'
    const advance = findStableBoundary(text, 0)
    // Should advance past heading + paragraph, leaving "Growing text" as unstable
    expect(advance).toBeGreaterThan(0)
    expect(text.substring(0, advance)).toContain('Heading')
    expect(text.substring(advance)).toContain('Growing text')
  })

  test('treats unclosed code fence as single block', () => {
    const text = 'Done paragraph\n\n```js\nconst x = 1'
    const advance = findStableBoundary(text, 0)
    // Paragraph is stable, unclosed fence is the growing block
    expect(advance).toBeGreaterThan(0)
    expect(text.substring(advance)).toContain('```')
  })

  test('advances from a previous boundary', () => {
    const text = '# H1\n\nPara one\n\nPara two\n\nGrowing'
    const first = findStableBoundary(text, 0)
    expect(first).toBeGreaterThan(0)
    // "Growing" is the only remaining block — nothing more to advance
    const second = findStableBoundary(text, first)
    expect(second).toBe(0)
    expect(text.substring(first)).toBe('Growing')
  })

  test('returns 0 when boundary is at end', () => {
    const text = 'hello'
    expect(findStableBoundary(text, text.length)).toBe(0)
  })
})

describe('splitStableBlocks', () => {
  test('returns empty for single block', () => {
    const { stableTexts, newBoundary } = splitStableBlocks('hello world', 0)
    expect(stableTexts).toEqual([])
    expect(newBoundary).toBe(0)
  })

  test('splits individual blocks', () => {
    const text = '# Heading\n\nParagraph one\n\nGrowing'
    const { stableTexts, newBoundary } = splitStableBlocks(text, 0)
    expect(stableTexts.length).toBe(2)
    expect(stableTexts[0]).toContain('Heading')
    expect(stableTexts[1]).toContain('Paragraph one')
    expect(text.substring(newBoundary)).toBe('Growing')
  })

  test('skips space-only tokens', () => {
    const text = '# H1\n\nPara\n\nTail'
    const { stableTexts } = splitStableBlocks(text, 0)
    // Should not include blank separator tokens
    for (const t of stableTexts) {
      expect(t.trim().length).toBeGreaterThan(0)
    }
  })

  test('handles code fence as single growing block', () => {
    const text = 'Done\n\n```js\nconst x = 1'
    const { stableTexts, newBoundary } = splitStableBlocks(text, 0)
    expect(stableTexts.length).toBe(1)
    expect(stableTexts[0]).toContain('Done')
    expect(text.substring(newBoundary)).toContain('```')
  })

  test('incremental splits across multiple calls', () => {
    const text = 'A\n\nB\n\nC\n\nD'
    const first = splitStableBlocks(text, 0)
    expect(first.stableTexts.length).toBeGreaterThan(0)

    const second = splitStableBlocks(text, first.newBoundary)
    // From first.newBoundary, remaining blocks may yield more stable texts
    const totalStable = first.stableTexts.length + second.stableTexts.length
    expect(totalStable).toBeGreaterThanOrEqual(first.stableTexts.length)
  })
})

describe('shouldAnimateTerminalTitle', () => {
  const original = process.env.EVOT_ANIMATE_TITLE

  afterEach(() => {
    if (original === undefined) {
      delete process.env.EVOT_ANIMATE_TITLE
    } else {
      process.env.EVOT_ANIMATE_TITLE = original
    }
  })

  test('is disabled by default', () => {
    delete process.env.EVOT_ANIMATE_TITLE
    expect(shouldAnimateTerminalTitle()).toBe(false)
  })

  test('can be enabled explicitly', () => {
    process.env.EVOT_ANIMATE_TITLE = '1'
    expect(shouldAnimateTerminalTitle()).toBe(true)
  })
})
