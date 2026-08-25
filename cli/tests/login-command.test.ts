import { describe, expect, test } from 'bun:test'
import type { AuthPollResult, LoginCodeResponse } from '../src/native/index.js'
import { handleLoginCommand, handleLogoutCommand, type ReplCommandContext } from '../src/term/repl-commands.js'

function createContext(): { ctx: ReplCommandContext; lines: { id: string; text: string }[] } {
  const lines: { id: string; text: string }[] = []
  return {
    lines,
    ctx: {
      agent: {} as ReplCommandContext['agent'],
      getSessionId: () => null,
      getCompactLines: () => [],
      getConfigInfo: () => null,
      commitSystem: (id, text) => { lines.push({ id, text }) },
      commitLines: () => {},
      requestRender: () => {},
    },
  }
}

function loginResponse(): LoginCodeResponse {
  return {
    code: 'CODE1',
    login_url: 'http://x/login?code=CODE1',
    expires_at: 111,
    expires_in_ms: 10_000,
    interval_ms: 1,
  }
}

describe('handleLoginCommand', () => {
  test('opens the login url and reports success', async () => {
    const { ctx, lines } = createContext()
    const opened: string[] = []
    const polls: AuthPollResult[] = [
      { status: 'pending' },
      { status: 'success', state: { user: { id: 'u', name: 'bo', email: 'b@x.dev' } } },
    ]
    let pollIndex = 0

    const ok = await handleLoginCommand(ctx, {
      fingerprint: async () => 'fp',
      begin: async () => loginResponse(),
      poll: async () => polls[Math.min(pollIndex++, polls.length - 1)]!,
      openBrowser: (url) => opened.push(url),
      sleep: async () => {},
      now: () => 0,
    })

    expect(ok).toBe(true)
    expect(opened).toEqual(['http://x/login?code=CODE1'])
    expect(lines.map(line => line.id)).toEqual(['sys-login', 'sys-login-url', 'sys-login-ok'])
    expect(lines.at(-1)?.text).toContain('logged in as bo <b@x.dev>')
    expect(lines.at(-1)?.text).toContain('free models synced')
  })

  test('reports denial and timeout without claiming success', async () => {
    const denied = createContext()
    expect(await handleLoginCommand(denied.ctx, {
      fingerprint: async () => 'fp',
      begin: async () => loginResponse(),
      poll: async () => ({ status: 'denied' }),
      openBrowser: () => {},
      sleep: async () => {},
      now: () => 0,
    })).toBe(false)
    expect(denied.lines.at(-1)?.id).toBe('sys-login-err')
    expect(denied.lines.at(-1)?.text).toContain('login denied')

    const timedOut = createContext()
    expect(await handleLoginCommand(timedOut.ctx, {
      fingerprint: async () => 'fp',
      begin: async () => loginResponse(),
      poll: async () => ({ status: 'expired' }),
      openBrowser: () => {},
      sleep: async () => {},
      now: () => 0,
    })).toBe(false)
    expect(timedOut.lines.at(-1)?.text).toContain('timed out')
  })

  test('surfaces begin failures in the TUI', async () => {
    const { ctx, lines } = createContext()
    const ok = await handleLoginCommand(ctx, {
      fingerprint: async () => 'fp',
      begin: async () => { throw new Error('offline') },
      poll: async () => ({ status: 'pending' }),
      openBrowser: () => {},
      sleep: async () => {},
      now: () => 0,
    })
    expect(ok).toBe(false)
    expect(lines.at(-1)?.id).toBe('sys-login-err')
    expect(lines.at(-1)?.text).toContain('offline')
  })
})

describe('handleLogoutCommand', () => {
  test('logs out the current user', async () => {
    const { ctx, lines } = createContext()
    let loggedOut = false
    const ok = await handleLogoutCommand(ctx, {
      whoami: async () => ({ id: 'u', name: 'bo', email: 'b@x.dev' }),
      logout: async () => { loggedOut = true },
    })
    expect(ok).toBe(true)
    expect(loggedOut).toBe(true)
    expect(lines).toEqual([
      { id: 'sys-logout-ok', text: '  ✓ logged out bo <b@x.dev>' },
    ])
  })

  test('does nothing when not logged in', async () => {
    const { ctx, lines } = createContext()
    let loggedOut = false
    const ok = await handleLogoutCommand(ctx, {
      whoami: async () => null,
      logout: async () => { loggedOut = true },
    })
    expect(ok).toBe(false)
    expect(loggedOut).toBe(false)
    expect(lines.at(-1)?.text).toContain('not logged in')
  })

  test('surfaces logout failures in the TUI', async () => {
    const { ctx, lines } = createContext()
    const ok = await handleLogoutCommand(ctx, {
      whoami: async () => ({ id: 'u', name: 'bo', email: 'b@x.dev' }),
      logout: async () => { throw new Error('disk full') },
    })
    expect(ok).toBe(false)
    expect(lines.at(-1)?.id).toBe('sys-logout-err')
    expect(lines.at(-1)?.text).toContain('disk full')
  })
})
