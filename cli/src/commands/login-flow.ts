import type { AuthPollResult, LoginCodeResponse } from '../native/index.js'

export interface LoginDeps {
  begin: (serverUrl: string, fingerprint: string) => Promise<LoginCodeResponse>
  poll: (serverUrl: string, code: string, expiresAt: number) => Promise<AuthPollResult>
  sleep: (ms: number) => Promise<void>
  now: () => number
}

export type LoginOutcome =
  | { status: 'success'; user: { name: string; email: string }; syncError?: string }
  | { status: 'timeout' }
  | { status: 'denied' }

export const defaultDeps: LoginDeps = {
  begin: async () => {
    throw new Error('default deps must not be called')
  },
  poll: async () => {
    throw new Error('default deps must not be called')
  },
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  now: () => Date.now(),
}

/**
 * Poll until approved/expired/denied. Deadline is computed from `deps.now()`,
 * so tests can advance a fake clock instead of really sleeping.
 */
export async function runLoginPolling(
  deps: LoginDeps,
  serverUrl: string,
  fingerprint: string,
): Promise<{ outcome: LoginOutcome; begin?: LoginCodeResponse }> {
  const begin = await deps.begin(serverUrl, fingerprint)
  const deadline = deps.now() + begin.expires_in_ms

  for (;;) {
    if (deps.now() >= deadline) return { outcome: { status: 'timeout' }, begin }
    await deps.sleep(begin.interval_ms ?? 2000)
    const result = await deps.poll(serverUrl, begin.code, begin.expires_at)
    switch (result.status) {
      case 'success':
        return {
          outcome: {
            status: 'success',
            user: result.state.user,
            syncError: result.sync_error,
          },
          begin,
        }
      case 'denied':
        return { outcome: { status: 'denied' }, begin }
      case 'expired':
        return { outcome: { status: 'timeout' }, begin }
    }
    if (deps.now() >= deadline) return { outcome: { status: 'timeout' }, begin }
  }
}
