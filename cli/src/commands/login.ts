import { exec } from 'node:child_process'

import { authBegin, authPoll } from '../native/index.js'
import { defaultDeps, runLoginPolling, type LoginDeps, type LoginOutcome } from './login-flow.js'

const DEFAULT_SERVER = process.env.EVOT_SERVER_URL ?? 'https://auto.evot.ai'

export async function runLogin(): Promise<boolean> {
  console.log('\nevot login\n')
  const fingerprint = await getFingerprint()

  let begin: LoginCodeResponse
  try {
    begin = await authBegin(DEFAULT_SERVER, fingerprint)
  } catch (err) {
    console.error(`  ✗ cannot reach evot server (${DEFAULT_SERVER})`)
    console.error(`    ${err instanceof Error ? err.message : err}`)
    process.exit(1)
  }
  // Open the browser once the login URL is known.
  let url: string | null = begin.login_url
  let opened = false
  const deps: LoginDeps = {
    ...defaultDeps,
    begin: async () => begin,
    poll: authPoll,
  }
  const pollWithOpen = async (server: string, code: string, expiresAt: number) => {
    if (url && !opened) {
      openBrowser(url)
      opened = true
      console.log(`Open this URL in your browser to log in:\n\n  ${url}\n`)
    }
    return authPoll(server, code, expiresAt)
  }

  const { outcome } = await runLoginPolling(
    { ...deps, poll: pollWithOpen },
    DEFAULT_SERVER,
    fingerprint,
  )
  report(outcome)
  return outcome.status === 'success'
}

function report(outcome: LoginOutcome): void {
  switch (outcome.status) {
    case 'success':
      console.log(`  ✓ logged in as ${outcome.user.name} <${outcome.user.email}>`)
      if (outcome.syncError) console.warn(`  ⚠ model sync failed: ${outcome.syncError}`)
      else console.log('  ✓ free models synced')
      break
    case 'denied':
      console.error('  ✗ login denied')
      break
    case 'timeout':
      console.error('  ✗ login timed out, try again')
      break
  }
}

export async function runLogout(): Promise<void> {
  const { authLogout } = await import('../native/index.js')
  await authLogout()
  console.log('  ✓ logged out')
}

export async function runWhoami(): Promise<number> {
  const { authWhoami } = await import('../native/index.js')
  const user = await authWhoami()
  if (!user) {
    console.log('  not logged in (run `evot login`, or configure a provider with an API key)')
    return 1
  }
  console.log(`  ${user.name} <${user.email}> (${user.id})`)
  return 0
}

async function getFingerprint(): Promise<string> {
  const { createHash } = await import('node:crypto')
  const os = await import('node:os')
  return createHash('sha256')
    .update(`${os.hostname()}:${os.platform()}:${os.arch()}:${os.userInfo().username}`)
    .digest('hex')
    .slice(0, 32)
}

function openBrowser(url: string): void {
  const cmd = process.platform === 'darwin' ? 'open' : process.platform === 'win32' ? 'start ""' : 'xdg-open'
  exec(`${cmd} "${url}"`, () => {})
}
