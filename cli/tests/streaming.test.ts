import { afterEach, describe, expect, test } from 'bun:test'
import { renderStreamingText, shouldAnimateTerminalTitle } from '../src/utils/streaming.js'

describe('renderStreamingText', () => {
  test('keeps markdown markers untouched while streaming', () => {
    expect(renderStreamingText('**bold** `code`')).toBe('**bold** `code`')
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
