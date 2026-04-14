import { afterEach, describe, expect, test } from 'bun:test'
import stripAnsi from 'strip-ansi'
import { renderStreamingText, shouldAnimateTerminalTitle, shouldRefreshStreamingMarkdown, splitStreamingMarkdown } from '../src/utils/streaming.js'

describe('renderStreamingText', () => {
  test('renders markdown while streaming', () => {
    const rendered = stripAnsi(renderStreamingText('**bold** `code`'))
    expect(rendered).toContain('bold')
    expect(rendered).toContain('code')
    expect(rendered).not.toContain('**')
  })

  test('normalizes unclosed code fences during streaming', () => {
    const rendered = stripAnsi(renderStreamingText('```ts\nconst x = 1'))
    expect(rendered).toContain('const x = 1')
    expect(rendered).not.toContain('```')
  })
})

describe('splitStreamingMarkdown', () => {
  test('advances a stable prefix and leaves the final growing block unstable', () => {
    const text = '# Title\n\n- one\n- two'
    const result = splitStreamingMarkdown(text, '')

    expect(result.stablePrefix).toBe('# Title\n\n')
    expect(result.unstableSuffix).toBe('- one\n- two')
  })

  test('reuses an existing stable prefix when text only appends to the tail', () => {
    const prev = '# Title\n\n'
    const text = '# Title\n\n- one\n- two\n- three'
    const result = splitStreamingMarkdown(text, prev)

    expect(result.stablePrefix).toBe(prev)
    expect(result.unstableSuffix).toBe('- one\n- two\n- three')
  })
})

describe('shouldRefreshStreamingMarkdown', () => {
  test('refreshes immediately for the first frame', () => {
    expect(shouldRefreshStreamingMarkdown(0, 100, 120)).toBe(true)
  })

  test('throttles intermediate frames inside the refresh interval', () => {
    expect(shouldRefreshStreamingMarkdown(100, 180, 120)).toBe(false)
  })

  test('refreshes after the interval elapses', () => {
    expect(shouldRefreshStreamingMarkdown(100, 221, 120)).toBe(true)
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
