#!/usr/bin/env bun
/**
 * evot CLI — TypeScript entry point.
 */

import { createAgent, parseArgs } from './cli.js'

async function main() {
  const rawArgs = process.argv.slice(2)
  const opts = await parseArgs(rawArgs)

  // Apply a background-staged download before any interactive path comes up.
  // A verified archive is already on disk, so this is a local copy-and-swap —
  // the whole point of staging was to keep it off the network. Interactive
  // commands only: `/update` manages its own lifecycle, and one-shot paths
  // (whoami, logout) must not pay for it.
  //
  // The swap only changes files. This process already mapped the old compiled
  // bundle and its native binding, so it hands over to the new executable via
  // execve instead of continuing — otherwise install bookkeeping says v(new)
  // while the session runs v(old).
  if (opts.command === 'repl' || opts.command === 'login' || opts.command === 'prompt') {
    try {
      const { applyStagedOnStartup, execIntoInstalledUpdate } = await import('./update/index.js')
      const { version } = await import('./native/index.js')
      const applied = await applyStagedOnStartup(version())
      if (applied) execIntoInstalledUpdate(applied)
    } catch { /* never block launch on update bookkeeping */ }
  }

  // Present when a re-exec just handed this process the new version. Reported
  // here, by the process that is genuinely running it.
  try {
    const { takeAppliedUpdate, reportAppliedUpdate } = await import('./update/index.js')
    const applied = takeAppliedUpdate()
    if (applied) reportAppliedUpdate(applied, opts.command)
  } catch { /* never block launch on update bookkeeping */ }

  if (opts.command === 'repl' || opts.command === 'login' || opts.command === 'prompt') {
    try {
      const { startOfficialSkillSync } = await import('./commands/skill.js')
      void startOfficialSkillSync().catch(() => { /* never block launch on skill sync */ })
    } catch { /* never block launch on skill sync bookkeeping */ }
  }

  switch (opts.command) {
    case 'serve': {
      const { startServer } = await import('./native/index.js')
      await startServer(opts.port, opts.model, opts.envFile)
      break
    }

    case 'prompt': {
      const { runPrompt } = await import('./prompt.js')
      await runPrompt(opts)
      break
    }

    case 'login': {
      const { runLogin } = await import('./commands/login.js')
      const loggedIn = await runLogin()
      // Login completed → drop straight into the REPL so the user lands in
      // the product instead of back at the shell. Failures exit inside runLogin.
      if (!loggedIn) { process.exitCode = 1; break }
      const agent = await createAgent(opts)
      const { startRepl } = await import('./term/repl.js')
      await startRepl({
        agent,
        resumeSessionId: opts.resume,
        continueLatest: opts.continueLatest,
        serverPort: opts.port,
        envFile: opts.envFile,
      })
      break
    }

    case 'logout': {
      const { runLogout } = await import('./commands/login.js')
      await runLogout()
      break
    }

    case 'whoami': {
      const { runWhoami } = await import('./commands/login.js')
      process.exitCode = await runWhoami()
      break
    }

    case 'update': {
      const { runUpdate } = await import('./update/index.js')
      const { version } = await import('./native/index.js')
      console.log('  checking for updates...')
      const result = await runUpdate(version())
      switch (result.kind) {
        case 'up_to_date':
          console.log(
            result.staleReason
              ? `  ✓ evot is up to date, per the last successful check (${result.staleReason}).`
              : '  ✓ evot is up to date.',
          )
          // Only present alongside a stale answer, where the route explains it.
          if (result.proxy) console.log(`    ${result.proxy}`)
          break
        case 'updated': {
          console.log(`  ✓ updated ${result.from} → ${result.to}`)
          if (result.notes && result.notes.length > 0) {
            console.log('')
            console.log(`  What's new in ${result.to}:`)
            for (const note of result.notes) {
              console.log(`    • ${note}`)
            }
          }
          break
        }
        case 'error': console.error(`  ✗ ${result.message}`)
          if (result.proxy) console.error(`    ${result.proxy}`)
          process.exit(1)
      }
      break
    }

    case 'repl':
    default: {
      const agent = await createAgent(opts)
      const { startRepl } = await import('./term/repl.js')
      await startRepl({
        agent,
        resumeSessionId: opts.resume,
        continueLatest: opts.continueLatest,
        serverPort: opts.port,
        envFile: opts.envFile,
      })
      break
    }
  }
}

main().catch((err: any) => {
  console.error(`Failed to initialize: ${err?.message ?? err}`)
  process.exit(1)
})
