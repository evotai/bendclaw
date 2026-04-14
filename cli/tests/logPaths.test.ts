import { describe, expect, test } from 'bun:test'
import { logDirPath, sessionLogPath, sessionTranscriptPath } from '../src/utils/logPaths.js'

describe('log path helpers', () => {
  test('builds both session log and transcript paths', () => {
    const home = '/Users/sundy'
    const sid = '019d8a14-58d0-7572-8de8-ca3f8a0fd777'

    expect(logDirPath(home)).toBe('/Users/sundy/.evotai/logs')
    expect(sessionLogPath(home, sid)).toBe(
      '/Users/sundy/.evotai/logs/019d8a14-58d0-7572-8de8-ca3f8a0fd777.log',
    )
    expect(sessionTranscriptPath(home, sid)).toBe(
      '/Users/sundy/.evotai/sessions/019d8a14-58d0-7572-8de8-ca3f8a0fd777/transcript.jsonl',
    )
  })
})
