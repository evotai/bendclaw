import React from 'react'
import { Agent } from '../native/index.js'
import type { AppState } from '../state/app.js'
import { createInitialState } from '../state/app.js'
import type { OutputLine } from '../render/output.js'
import { resolveCommand } from './index.js'
import { skillList, skillInstall, skillRemove } from './skill.js'
import { resumeSession, syncProvider } from '../repl/session.js'
import { runLogTurn } from '../repl/actions.js'
import { pushSystem, type SystemMsg } from '../repl/messages.js'

export interface CommandContext {
  agent: Agent
  state: AppState
  setState: React.Dispatch<React.SetStateAction<AppState>>
  setSystem: React.Dispatch<React.SetStateAction<SystemMsg[]>>
  setOutputLines: React.Dispatch<React.SetStateAction<OutputLine[]>>
  setPendingText: React.Dispatch<React.SetStateAction<string>>
  setShowHelp: React.Dispatch<React.SetStateAction<boolean>>
  setResumeSessions: React.Dispatch<React.SetStateAction<import('../native/index.js').SessionMeta[] | null>>
  setPlanning: React.Dispatch<React.SetStateAction<boolean>>
  setShowModelSelector: React.Dispatch<React.SetStateAction<boolean>>
  setLogMode: React.Dispatch<React.SetStateAction<import('../native/index.js').ForkedAgent | null>>
  configInfo: import('../native/index.js').ConfigInfo | undefined
  abortCurrentStream: () => void
  exit: () => void
}

export async function handleSlashCommand(input: string, ctx: CommandContext) {
  const { agent, state, setState, setSystem, setOutputLines, setPendingText, setShowHelp, setResumeSessions, setPlanning, setShowModelSelector, setLogMode, configInfo, abortCurrentStream, exit } = ctx
  const resolved = resolveCommand(input)

  if (resolved.kind === 'unknown') {
    pushSystem(setSystem, 'error', `Unknown command: ${input}`)
    pushSystem(setSystem, 'info', 'Type /help for available commands')
    return
  }

  if (resolved.kind === 'ambiguous') {
    pushSystem(setSystem, 'warn', `Ambiguous command: ${resolved.candidates.join(', ')}`)
    pushSystem(setSystem, 'info', 'Type more characters or /help for commands')
    return
  }

  const { name, args } = resolved

  switch (name) {
    case '/help':
      setShowHelp(true)
      break
    case '/exit':
      abortCurrentStream()
      exit()
      break
    case '/clear':
      abortCurrentStream()
      process.stdout.write('\x1b[2J\x1b[H')
      setState((prev) => ({ ...prev, messages: [] }))
      setOutputLines([])
      pushSystem(setSystem, 'info', 'Messages cleared.')
      break
    case '/new':
      abortCurrentStream()
      setState((prev) => ({ ...createInitialState(prev.model, prev.cwd), verbose: prev.verbose }))
      pushSystem(setSystem, 'info', 'New session started.')
      break
    case '/model': {
      const arg = args.trim()
      if (arg === 'n') {
        const models = configInfo?.availableModels ?? [state.model]
        if (models.length <= 1) {
          pushSystem(setSystem, 'info', 'Only one model available.')
        } else {
          const idx = models.indexOf(state.model)
          const next = models[(idx + 1) % models.length]!
          agent.model = next
          syncProvider(agent, next, configInfo)
          setState((prev) => ({ ...prev, model: next }))
          pushSystem(setSystem, 'info', `Model → ${next}`)
        }
      } else if (arg) {
        agent.model = arg
        syncProvider(agent, arg, configInfo)
        setState((prev) => ({ ...prev, model: arg }))
        pushSystem(setSystem, 'info', `Model → ${arg}`)
      } else {
        setShowModelSelector(true)
      }
      break
    }
    case '/verbose':
      setState((prev) => ({ ...prev, verbose: !prev.verbose }))
      pushSystem(setSystem, 'info', `Verbose mode ${state.verbose ? 'off' : 'on'}`)
      break
    case '/resume': {
      abortCurrentStream()
      try {
        const allSessions = await agent.listSessions(20)
        if (allSessions.length === 0) {
          pushSystem(setSystem, 'info', 'No sessions found.')
          break
        }
        const cwdSessions = allSessions.filter((s) => s.cwd === agent.cwd)
        const sessions = cwdSessions.length > 0 ? cwdSessions : allSessions
        if (args.trim()) {
          const prefix = args.trim()
          const matches = allSessions.filter((s) => s.session_id === prefix || s.session_id.startsWith(prefix))
          if (matches.length === 0) {
            pushSystem(setSystem, 'error', `Session not found: ${prefix}`)
          } else if (matches.length > 1) {
            pushSystem(setSystem, 'error', `Ambiguous session id: ${prefix}`)
          } else {
            await resumeSession(agent, matches[0]!, setState, setSystem, setOutputLines)
          }
        } else {
          setResumeSessions(sessions.slice(0, 20))
        }
      } catch (err: any) {
        pushSystem(setSystem, 'error', `Failed to list sessions: ${err?.message ?? err}`)
      }
      break
    }
    case '/plan':
      setPlanning(true)
      pushSystem(setSystem, 'info', 'Planning mode on — read-only tools only. Use /act to resume execution.')
      break
    case '/act':
      setPlanning(false)
      pushSystem(setSystem, 'info', 'Action mode on — full tool set restored.')
      break
    case '/env': {
      const sub = args.trim()
      if (!sub) {
        const vars = agent.listVariables()
        if (vars.length > 0) {
          const lines = vars.map((v) => `  ${v.key.padEnd(28)} ${v.value.slice(0, 2)}****${v.value.slice(-2)}`)
          pushSystem(setSystem, 'info', `Agent variables:\n${lines.join('\n')}`)
        } else {
          pushSystem(setSystem, 'info', 'No agent variables set.')
        }
      } else if (sub.startsWith('set ')) {
        const kv = sub.slice(4).trim()
        const eqIdx = kv.indexOf('=')
        if (eqIdx <= 0) {
          pushSystem(setSystem, 'error', 'Usage: /env set KEY=VALUE')
        } else {
          const key = kv.slice(0, eqIdx)
          const val = kv.slice(eqIdx + 1)
          try {
            await agent.setVariable(key, val)
            pushSystem(setSystem, 'info', `Set ${key}`)
          } catch (err: any) {
            pushSystem(setSystem, 'error', `Failed: ${err?.message ?? err}`)
          }
        }
      } else if (sub.startsWith('del ')) {
        const key = sub.slice(4).trim()
        if (!key) {
          pushSystem(setSystem, 'error', 'Usage: /env del KEY')
        } else {
          try {
            const removed = await agent.deleteVariable(key)
            pushSystem(setSystem, 'info', removed ? `Deleted ${key}` : `${key} not found`)
          } catch (err: any) {
            pushSystem(setSystem, 'error', `Failed: ${err?.message ?? err}`)
          }
        }
      } else if (sub.startsWith('load ')) {
        const filePath = sub.slice(5).trim()
        try {
          const { readFileSync } = await import('fs')
          const content = readFileSync(filePath, 'utf-8')
          let count = 0
          for (const line of content.split('\n')) {
            const trimmed = line.trim()
            if (!trimmed || trimmed.startsWith('#')) continue
            const clean = trimmed.startsWith('export ') ? trimmed.slice(7) : trimmed
            const eq = clean.indexOf('=')
            if (eq > 0) {
              const k = clean.slice(0, eq)
              let v = clean.slice(eq + 1)
              if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) {
                v = v.slice(1, -1)
              }
              await agent.setVariable(k, v)
              count++
            }
          }
          pushSystem(setSystem, 'info', `Loaded ${count} vars from ${filePath}`)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `Failed to load: ${err?.message ?? err}`)
        }
      } else {
        pushSystem(setSystem, 'error', 'Usage: /env [set K=V | del K | load FILE]')
      }
      break
    }
    case '/log': {
      const { join } = await import('path')
      const { homedir } = await import('os')
      const logDir = join(homedir(), '.evotai', 'logs')
      const sid = state.sessionId
      const query = args.trim()

      if (query.startsWith('up')) {
        // /log up [session_id_prefix]
        const upArg = query.slice(2).trim()
        const targetSid = upArg || sid
        if (!targetSid) {
          pushSystem(setSystem, 'error', 'No active session. Usage: /log up [session_id]')
          break
        }
        let resolvedSid = targetSid
        if (upArg) {
          try {
            const allSessions = await agent.listSessions(100)
            const matches = allSessions.filter((s) => s.session_id === upArg || s.session_id.startsWith(upArg))
            if (matches.length === 0) {
              pushSystem(setSystem, 'error', `Session not found: ${upArg}`)
              break
            }
            if (matches.length > 1) {
              pushSystem(setSystem, 'error', `Ambiguous session id: ${upArg} (${matches.length} matches)`)
              break
            }
            resolvedSid = matches[0]!.session_id
          } catch (err: any) {
            pushSystem(setSystem, 'error', `Failed to list sessions: ${err?.message ?? err}`)
            break
          }
        }
        pushSystem(setSystem, 'info', `  packing session ${resolvedSid.slice(0, 8)}...`)
        try {
          const { logPut } = await import('./log-share.js')
          const result = await logPut(resolvedSid)
          pushSystem(setSystem, 'info', `  uploaded. share this link:\n  ${result.url}\n  ⏳ link expires in 60 minutes`)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `Export failed: ${err?.message ?? err}`)
        }
      } else if (query.startsWith('dl ')) {
        // /log dl <url#password>
        const dlUrl = query.slice(3).trim()
        if (!dlUrl) {
          pushSystem(setSystem, 'error', 'Usage: /log dl <url#password>')
          break
        }
        pushSystem(setSystem, 'info', '  downloading and importing...')
        try {
          const { logGet } = await import('./log-share.js')
          const result = await logGet(dlUrl)
          pushSystem(setSystem, 'info', `  imported session: ${result.sessionId}\n  resume with: /resume ${result.sessionId.slice(0, 8)}`)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `Import failed: ${err?.message ?? err}`)
        }
      } else if (!query) {
        if (sid) pushSystem(setSystem, 'info', `Log: ${join(logDir, `${sid}.screen.log`)}`)
        else pushSystem(setSystem, 'info', `Log dir: ${logDir} (no active session)`)
      } else if (!sid) {
        pushSystem(setSystem, 'error', 'No active session to analyze.')
      } else {
        const logPath = join(logDir, `${sid}.screen.log`)
        const systemPrompt = [
          'You are in a temporary log analysis session.',
          'This session is not persisted and does not affect the main session context.',
          '',
          `Log file to analyze:\n${logPath}`,
          '',
          'Rules:',
          '- Read relevant log sections before answering; do not guess',
          '- Prefer partial reads; avoid loading the entire file at once',
          '- Use search to locate key information when needed',
          '- Do not modify any files',
        ].join('\n')
        try {
          const forked = agent.fork(systemPrompt)
          setLogMode(forked)
          pushSystem(setSystem, 'info', `  [log mode] analyzing: ${logPath}\n  not persisted. type /done to return.`)
          runLogTurn(forked, query, setOutputLines, setPendingText, setState)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `Fork failed: ${err?.message ?? err}`)
        }
      }
      break
    }
    case '/skill': {
      const sub = args.trim()
      if (!sub || sub === 'list') {
        pushSystem(setSystem, 'info', skillList())
      } else if (sub.startsWith('install ')) {
        const source = sub.slice(8).trim()
        if (!source) {
          pushSystem(setSystem, 'error', 'Usage: /skill install <owner/repo>')
          break
        }
        pushSystem(setSystem, 'info', `  cloning ${source}`)
        try {
          const forked = agent.fork('You analyze skills and provide setup guides.')
          const result = await skillInstall(source, forked, (msg, level) => {
            pushSystem(setSystem, level, msg)
          })
          if (result) pushSystem(setSystem, 'info', result)
        } catch (err: any) {
          pushSystem(setSystem, 'error', `  install failed: ${err?.message ?? err}`)
        }
      } else if (sub.startsWith('remove ')) {
        const name = sub.slice(7).trim()
        if (!name) pushSystem(setSystem, 'error', 'Usage: /skill remove <name>')
        else pushSystem(setSystem, 'info', skillRemove(name))
      } else {
        pushSystem(setSystem, 'error', 'Usage: /skill [list | install <source> | remove <name>]')
      }
      break
    }
    case '/update': {
      pushSystem(setSystem, 'info', '  checking for updates...')
      try {
        const { runUpdate } = await import('../update/index.js')
        const { version } = await import('../native/index.js')
        const result = await runUpdate(version())
        switch (result.kind) {
          case 'up_to_date':
            pushSystem(setSystem, 'info', '  ✓ evot is up to date.')
            break
          case 'updated':
            pushSystem(setSystem, 'info', `  ✓ updated ${result.from} → ${result.to}. restart evot to apply.`)
            break
          case 'error':
            pushSystem(setSystem, 'error', `  ✗ ${result.message}`)
            break
        }
      } catch (err: any) {
        pushSystem(setSystem, 'error', `  ✗ update failed: ${err?.message ?? err}`)
      }
      break
    }
    default:
      pushSystem(setSystem, 'error', `Unhandled command: ${name}`)
  }
}
