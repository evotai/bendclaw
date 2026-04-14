import { describe, expect, test } from 'bun:test'
import { getFreezeInputMode } from '../src/utils/freezeMode.js'

describe('getFreezeInputMode', () => {
  test('disables input and raw mode while frozen', () => {
    expect(getFreezeInputMode(true, true)).toEqual({
      shouldCaptureInput: false,
      shouldUseRawMode: false,
      resumeOnLineInput: true,
    })
  })

  test('keeps input and raw mode enabled while interactive', () => {
    expect(getFreezeInputMode(true, false)).toEqual({
      shouldCaptureInput: true,
      shouldUseRawMode: true,
      resumeOnLineInput: false,
    })
  })

  test('keeps everything disabled when prompt is inactive for other overlays', () => {
    expect(getFreezeInputMode(false, false)).toEqual({
      shouldCaptureInput: false,
      shouldUseRawMode: false,
      resumeOnLineInput: false,
    })
  })
})
