import chalk from 'chalk'
import type { Agent } from '../native/index.js'
import type { ConfigInfo } from '../native/index.js'
import { findLastAssistantMarkdown, findLastAssistantTurn } from '../session/assistant-markdown.js'
import { resolveSessionByPrefix } from './app/resume.js'
import type { OutputLine } from '../render/output.js'
import { defaultDeps, type LoginDeps } from '../commands/login-flow.js'

export interface ReplCommandContext {
  agent: Agent
  getSessionId: () => string | null
  getCompactLines: () => import('../render/output.js').OutputLine[]
  getConfigInfo: () => ConfigInfo | null
  commitSystem: (id: string, text: string, kind?: OutputLine['kind']) => void
  /** Commit a secret shown on screen only, erased after `delayMs`. */
  commitRevealed: (id: string, text: string, erasedText: string, delayMs: number) => void
  commitLines: (lines: OutputLine[]) => void
  requestRender: () => void
}

export function formatLogPaths(logPath: string | null, rendererPath: string | null = null): string | null {
  if (!logPath) return null
  const lines = [`  Log: ${logPath}`]
  if (rendererPath) lines.push(`  Renderer run: ${rendererPath}`)
  return lines.join('\n')
}

function failureText(label: string, err: unknown): string {
  const message = (err as { message?: string })?.message ?? String(err)
  return chalk.red(`  ${label}: ${message}`)
}

/** Distinguishes concurrent reveals; see the comment at its use site. */
let nextRevealId = 0

async function defaultLoginCommandDeps(): Promise<LoginCommandDeps> {
  const { deviceFingerprint, openLoginBrowser } = await import('../commands/login.js')
  const { authBegin, authPoll } = await import('../native/index.js')
  return {
    fingerprint: deviceFingerprint,
    begin: authBegin,
    poll: authPoll,
    openBrowser: openLoginBrowser,
  }
}

async function defaultLogoutCommandDeps(): Promise<LogoutCommandDeps> {
  const { authLogout, authWhoami } = await import('../native/index.js')
  return { whoami: authWhoami, logout: authLogout }
}

export async function handleCopyCommand(ctx: ReplCommandContext): Promise<void> {
  const last = findLastAssistantMarkdown(ctx.getCompactLines())
  if (!last) {
    ctx.commitSystem('sys-copy', '  No agent messages to copy yet.')
    return
  }
  try {
    const { copyToClipboard } = await import('../render/clipboard.js')
    await copyToClipboard(last.rawMarkdown)
    ctx.commitSystem('sys-copy', '  Copied last agent message (Markdown source) to clipboard')
  } catch (err) {
    ctx.commitSystem('sys-copy-err', failureText('Copy failed', err))
  }
}

export async function handleClipCommand(ctx: ReplCommandContext): Promise<void> {
  const last = findLastAssistantTurn(ctx.getCompactLines())
  if (!last) {
    ctx.commitSystem('sys-clip', '  No agent messages to clip yet.')
    return
  }
  try {
    const { clipMarkdown } = await import('../commands/clip.js')
    const result = clipMarkdown(last.rawMarkdown, {
      sessionId: ctx.getSessionId() ?? undefined,
      cwd: ctx.agent.cwd,
    })
    ctx.commitSystem('sys-clip', `  Clipped: ${result.path}`)
  } catch (err) {
    ctx.commitSystem('sys-clip-err', failureText('Clip failed', err))
  }
}

export async function handleShareCommand(ctx: ReplCommandContext, args: string): Promise<void> {
  const target = args.trim()
  const { importSharedSession, isSharedSessionUrl, shareSession } = await import('../commands/share.js')

  if (target && isSharedSessionUrl(target)) {
    ctx.commitSystem('sys-share-import', '  downloading and importing...')
    ctx.requestRender()
    try {
      const result = await importSharedSession(target)
      ctx.commitSystem('sys-share-import-ok', `  imported session: ${result.sessionId}\n  resume with: /resume ${result.sessionId.slice(0, 8)}`)
    } catch (err) {
      ctx.commitSystem('sys-share-err', failureText('Import failed', err))
    }
    return
  }

  let resolvedSid = ctx.getSessionId()
  if (target) {
    if (!/^[0-9a-f-]{1,36}$/i.test(target)) {
      ctx.commitSystem('sys-share-err', '  Usage: /share [session-id | url#password]')
      return
    }
    try {
      const sessions = await ctx.agent.listSessions(0)
      const resolved = resolveSessionByPrefix(sessions, target)
      if (resolved.kind === 'none') {
        ctx.commitSystem('sys-share-err', `  Session not found: ${target}`)
        return
      }
      if (resolved.kind === 'ambiguous') {
        ctx.commitSystem('sys-share-err', `  Ambiguous session id: ${target} (${resolved.matches.length} matches)`)
        return
      }
      resolvedSid = resolved.session.session_id
    } catch (err) {
      ctx.commitSystem('sys-share-err', failureText('Failed to list sessions', err))
      return
    }
  }

  if (!resolvedSid) {
    ctx.commitSystem('sys-share-err', '  No active session to share.')
    return
  }

  ctx.commitSystem('sys-share', `  packing session ${resolvedSid.slice(0, 8)}...`)
  ctx.requestRender()
  try {
    const result = await shareSession(resolvedSid)
    ctx.commitSystem('sys-share-url', `  uploaded. share this link:\n  ${result.url}\n  ⏳ link expires in 60 minutes`)
  } catch (err) {
    ctx.commitSystem('sys-share-err', failureText('Share failed', err))
  }
}

export async function handleSkillCommand(ctx: ReplCommandContext, args: string): Promise<void> {
  const sub = args.trim()
  const progress = (msg: string): void => {
    ctx.commitLines([{ id: `sys-skill-${Date.now()}`, kind: 'system', text: `  ${msg}` }])
    ctx.requestRender()
  }

  if (!sub || sub === 'list') {
    try {
      const { skillList } = await import('../commands/skill.js')
      ctx.commitSystem('sys-skill', skillList(ctx.agent.skillsDirs()))
    } catch {
      ctx.commitSystem('sys-skill-err', '  skill list unavailable')
    }
  } else if (sub === 'install' || sub.startsWith('install ')) {
    const source = sub.slice(7).trim()
    ctx.commitSystem('sys-skill-inst', `  installing ${source || 'official skills'}...`)
    ctx.requestRender()
    try {
      const { skillInstall } = await import('../commands/skill.js')
      const result = await skillInstall(source || undefined, { progress })
      ctx.commitSystem('sys-skill-done', result || '  nothing to install')
    } catch (err) {
      ctx.commitSystem('sys-skill-err', failureText('install failed', err))
    }
  } else if (sub === 'update' || sub.startsWith('update ')) {
    const name = sub.slice(6).trim()
    ctx.commitSystem('sys-skill-up', `  updating ${name || 'installed skills'}...`)
    ctx.requestRender()
    try {
      const { skillUpdate } = await import('../commands/skill.js')
      const result = await skillUpdate(name || undefined, { progress })
      ctx.commitSystem('sys-skill-done', result || '  nothing to update')
    } catch (err) {
      ctx.commitSystem('sys-skill-err', failureText('update failed', err))
    }
  } else if (sub.startsWith('remove ')) {
    const name = sub.slice(7).trim()
    if (!name) {
      ctx.commitSystem('sys-skill-err', '  Usage: /skill remove <name>')
    } else {
      try {
        const { skillRemove } = await import('../commands/skill.js')
        ctx.commitSystem('sys-skill-rm', skillRemove(name))
      } catch {
        ctx.commitSystem('sys-skill-err', '  skill remove unavailable')
      }
    }
  } else {
    ctx.commitSystem(
      'sys-skill-err',
      '  Usage: /skill [list | install [name | source] | update [name] | remove <name>]',
    )
  }
  ctx.requestRender()
}

export interface LoginCommandDeps {
  fingerprint: () => Promise<string>
  begin: LoginDeps['begin']
  poll: LoginDeps['poll']
  openBrowser: (url: string) => void
  sleep?: LoginDeps['sleep']
  now?: LoginDeps['now']
}

/** In-REPL login: device-code flow, then the caller reloads model config. */
export async function handleLoginCommand(
  ctx: ReplCommandContext,
  injected?: LoginCommandDeps,
): Promise<boolean> {
  ctx.commitSystem('sys-login', '  starting login...')
  ctx.requestRender()

  try {
    const { DEFAULT_SERVER, runDeviceLogin } = await import('../commands/login-flow.js')
    const deps: LoginCommandDeps = injected ?? await defaultLoginCommandDeps()
    const { outcome } = await runDeviceLogin(
      {
        ...defaultDeps,
        begin: deps.begin,
        poll: deps.poll,
        sleep: deps.sleep ?? defaultDeps.sleep,
        now: deps.now ?? defaultDeps.now,
      },
      DEFAULT_SERVER,
      await deps.fingerprint(),
      (url) => {
        deps.openBrowser(url)
        ctx.commitSystem('sys-login-url', `  Open this URL to log in:\n\n  ${url}`)
        ctx.requestRender()
      },
    )

    switch (outcome.status) {
      case 'success': {
        const lines = [`  ✓ logged in as ${outcome.user.name} <${outcome.user.email}>`]
        if (outcome.syncError) lines.push(`  ⚠ model sync failed: ${outcome.syncError}`)
        else lines.push('  ✓ free models synced')
        ctx.commitSystem('sys-login-ok', lines.join('\n'))
        return true
      }
      case 'denied':
        ctx.commitSystem('sys-login-err', chalk.red('  ✗ login denied'))
        return false
      case 'timeout':
        ctx.commitSystem('sys-login-err', chalk.red('  ✗ login timed out, try again'))
        return false
    }
  } catch (err) {
    ctx.commitSystem('sys-login-err', failureText('login failed', err))
    return false
  }
}

export interface LogoutCommandDeps {
  whoami: () => Promise<{ id: string; name: string; email: string } | null>
  logout: () => Promise<void>
}

/** In-REPL logout: drop cloud auth, then the caller reloads model config. */
export async function handleLogoutCommand(
  ctx: ReplCommandContext,
  injected?: LogoutCommandDeps,
): Promise<boolean> {
  try {
    const deps = injected ?? await defaultLogoutCommandDeps()
    const existing = await deps.whoami()
    if (!existing) {
      ctx.commitSystem('sys-logout', '  not logged in')
      return false
    }
    await deps.logout()
    ctx.commitSystem('sys-logout-ok', `  ✓ logged out ${existing.name} <${existing.email}>`)
    return true
  } catch (err) {
    ctx.commitSystem('sys-logout-err', failureText('logout failed', err))
    return false
  }
}

export async function handleVersionCommand(ctx: ReplCommandContext): Promise<void> {
  const { version } = await import('../native/index.js')
  ctx.commitSystem('sys-version', `  evot v${version()}`)
}

export async function handleUpdateCommand(ctx: ReplCommandContext): Promise<void> {
  ctx.commitSystem('sys-upd', '  checking for updates...')
  ctx.requestRender()
  try {
    const { runUpdate } = await import('../update/index.js')
    const { version } = await import('../native/index.js')
    const result = await runUpdate(version())
    switch (result.kind) {
      case 'up_to_date':
        ctx.commitSystem(
          'sys-upd-ok',
          [
            result.staleReason
              ? `  ✓ evot is up to date, per the last successful check (${result.staleReason}).`
              : '  ✓ evot is up to date.',
            // Only present alongside a stale answer, where the route explains it.
            ...(result.proxy ? [`    ${result.proxy}`] : []),
          ].join('\n'),
        )
        break
      case 'updated': {
        const lines: string[] = [`  ✓ updated ${result.from} → ${result.to}. /restart to apply.`]
        if (result.notes && result.notes.length > 0) {
          lines.push('')
          lines.push(`  What's new in ${result.to}:`)
          for (const note of result.notes) {
            lines.push(`    • ${note}`)
          }
        }
        ctx.commitSystem('sys-upd-ok', lines.join('\n'))
        break
      }
      case 'error':
        ctx.commitSystem(
          'sys-upd-err',
          chalk.red([`  ✗ ${result.message}`, ...(result.proxy ? [`    ${result.proxy}`] : [])].join('\n')),
        )
        break
    }
  } catch (err) {
    ctx.commitSystem('sys-upd-err', chalk.red(`  ✗ update failed: ${(err as { message?: string })?.message ?? err}`))
  }
}

export async function handleEnvCommand(ctx: ReplCommandContext, args: string): Promise<void> {
  const { runEnvCommand, parseRevealTarget, renderRevealed, REVEAL_ERASE_MS } = await import('../commands/env.js')
  const port = {
    list: () => ctx.agent.listVariables(),
    set: (key: string, value: string) => ctx.agent.setVariable(key, value),
    del: (key: string) => ctx.agent.deleteVariable(key),
    readFile: async (path: string) => {
      const { readFile } = await import('fs/promises')
      const { homedir } = await import('os')
      const expanded = path.startsWith('~/') ? `${homedir()}${path.slice(1)}` : path
      return readFile(expanded, 'utf8')
    },
  }
  try {
    // A reveal is committed differently from every other `/env` output: shown on
    // screen, withheld from the screen log, and erased on a timer. Everything
    // else is ordinary history.
    const revealKey = parseRevealTarget(args)
    const revealRow = revealKey ? port.list().find((row) => row.key === revealKey) : undefined
    if (revealRow) {
      const { text, erasedText } = renderRevealed(revealRow)
      // A fresh id per reveal. The erase finds its line by id, so a shared one
      // made the second reveal mask the first line twice and leave its own
      // value on screen for good.
      ctx.commitRevealed(`sys-env-reveal-${nextRevealId++}`, text, erasedText, REVEAL_ERASE_MS)
      ctx.requestRender()
      return
    }
    ctx.commitSystem('sys-env', await runEnvCommand(port, args))
  } catch (err) {
    ctx.commitSystem('sys-env-err', failureText('env failed', err))
  }
  ctx.requestRender()
}
