import { describe, test, expect } from 'bun:test'

import { runDeviceLogin, runLoginPolling, type LoginDeps } from '../src/commands/login-flow'
import type { AuthPollResult, LoginCodeResponse } from '../native/index.js'

function createFakeDeps(pollResults: AuthPollResult[]): {
  deps: LoginDeps
  calls: { polls: number; begins: number; slept: number[] }
} {
  let now = 0
  const state = { polls: 0, begins: 0, slept: [] as number[] }
  const deps: LoginDeps = {
    begin: async () => {
      state.begins += 1
      const response: LoginCodeResponse = {
        code: 'CODE1',
        login_url: 'http://x/login?code=CODE1',
        expires_at: 111,
        expires_in_ms: 10_000,
        interval_ms: 2000,
      }
      return response
    },
    poll: async (_url, code, expiresAt) => {
      expect(code).toBe('CODE1')
      expect(expiresAt).toBe(111)
      state.polls += 1
      return pollResults[Math.min(state.polls - 1, pollResults.length - 1)]
    },
    sleep: async (ms) => {
      state.slept.push(ms)
      now += ms
    },
    now: () => now,
  }
  return { deps, calls: state }
}

describe('runLoginPolling', () => {
  test('succeeds after pending polls', async () => {
    const { deps, calls } = createFakeDeps([
      { status: 'pending' },
      { status: 'pending' },
      { status: 'success', state: { user: { id: 'u', name: 'bo', email: 'b@x.dev' } } },
    ])
    const { outcome } = await runLoginPolling(deps, 'http://x', 'fp')
    expect(outcome.status).toBe('success')
    if (outcome.status === 'success') {
      expect(outcome.user.email).toBe('b@x.dev')
      expect(outcome.syncError).toBeUndefined()
    }
    expect(calls.begins).toBe(1)
    expect(calls.polls).toBe(3)
  })

  test('reports sync_error when model sync failed server-side', async () => {
    const { deps } = createFakeDeps([
      { status: 'success', state: { user: { id: 'u', name: 'bo', email: 'b@x.dev' } }, sync_error: 'offline' },
    ])
    const { outcome } = await runLoginPolling(deps, 'http://x', 'fp')
    expect(outcome.status).toBe('success')
    if (outcome.status === 'success') expect(outcome.syncError).toBe('offline')
  })

  test('times out when deadline passes with only pending responses', async () => {
    const { deps } = createFakeDeps([{ status: 'pending' }])
    const { outcome } = await runLoginPolling(deps, 'http://x', 'fp')
    expect(outcome.status).toBe('timeout')
  })

  test('maps expired to timeout and denied to denied', async () => {
    const expired = createFakeDeps([{ status: 'expired' }])
    const expiredOutcome = (await runLoginPolling(expired.deps, 'http://x', 'fp')).outcome
    expect(expiredOutcome.status).toBe('timeout')

    const denied = createFakeDeps([{ status: 'denied' }])
    const deniedOutcome = (await runLoginPolling(denied.deps, 'http://x', 'fp')).outcome
    expect(deniedOutcome.status).toBe('denied')
  })
})

describe('runDeviceLogin', () => {
  test('surfaces the login url before polling', async () => {
    const { deps, calls } = createFakeDeps([
      { status: 'success', state: { user: { id: 'u', name: 'bo', email: 'b@x.dev' } } },
    ])
    const urls: string[] = []
    const { outcome, begin } = await runDeviceLogin(deps, 'http://x', 'fp', (url) => urls.push(url))
    expect(urls).toEqual(['http://x/login?code=CODE1'])
    expect(begin.code).toBe('CODE1')
    expect(outcome.status).toBe('success')
    expect(calls.begins).toBe(1)
    expect(calls.polls).toBe(1)
  })
})
