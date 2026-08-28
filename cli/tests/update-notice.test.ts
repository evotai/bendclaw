import { describe, expect, test } from 'bun:test'
import { reportAppliedUpdate } from '../src/update/index.js'

/**
 * One-shot prompt runs are scripting surfaces: whatever lands on stdout gets
 * captured into files and JSON-lines parsers. The applied-update banner must
 * never ride stdout there — that is exactly how customers ended up with
 * "✓ evot updated to …" at the top of a saved output file.
 */
function captureRouting(command: string): { out: string[]; err: string[] } {
  const out: string[] = []
  const err: string[] = []
  const log = console.log
  const error = console.error
  console.log = (line?: unknown) => { out.push(String(line)) }
  console.error = (line?: unknown) => { err.push(String(line)) }
  try {
    reportAppliedUpdate('2026.8.28', command)
  } finally {
    console.log = log
    console.error = error
  }
  return { out, err }
}

describe('applied-update notice routing', () => {
  const line = '  ✓ evot updated to v2026.8.28 in the background; this session is running the new version.'

  test('prompt mode keeps stdout clean and uses stderr', () => {
    const { out, err } = captureRouting('prompt')
    expect(out).toEqual([])
    expect(err).toEqual([line])
  })

  test('interactive paths still announce on stdout', () => {
    for (const command of ['repl', 'login']) {
      const { out, err } = captureRouting(command)
      expect(out).toEqual([line])
      expect(err).toEqual([])
    }
  })
})
