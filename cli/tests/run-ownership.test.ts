import { describe, expect, test } from 'bun:test'
import { RunOwnership } from '../src/term/app/run-ownership.js'

describe('RunOwnership', () => {
  test('revokes an interrupted run before a replacement starts', () => {
    const ownership = new RunOwnership()
    const interrupted = ownership.begin()

    ownership.revoke()
    const replacement = ownership.begin()

    expect(ownership.owns(interrupted)).toBe(false)
    expect(ownership.owns(replacement)).toBe(true)
  })

  test('starting a new run prevents an older finally block from owning state', () => {
    const ownership = new RunOwnership()
    const first = ownership.begin()
    const second = ownership.begin()

    expect(ownership.owns(first)).toBe(false)
    expect(ownership.owns(second)).toBe(true)
  })
})
