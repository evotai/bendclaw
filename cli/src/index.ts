#!/usr/bin/env bun
/**
 * evot CLI — TypeScript entry point.
 */

import { startServer } from './native/index.js'
import { createAgent, parseArgs } from './cli.js'
import { runPrompt } from './prompt.js'

async function main() {
  const rawArgs = process.argv.slice(2)
  const opts = await parseArgs(rawArgs)

  switch (opts.command) {
    case 'serve':
      await startServer(opts.port, opts.model, opts.envFile)
      break

    case 'prompt':
      await runPrompt(opts)
      break

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
        case 'error': console.error(`  ✗ ${result.message}`); process.exit(1)
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
