#!/usr/bin/env bun
/**
 * evot CLI — TypeScript entry point.
 */

import React from 'react'
import { render } from 'ink'
import { startServer } from './native/index.js'
import { REPL } from './repl/REPL.js'
import { createAgent, parseArgs } from './cli.js'
import { runPrompt } from './prompt.js'

async function main() {
  const opts = parseArgs(process.argv.slice(2))

  switch (opts.command) {
    case 'serve':
      await startServer(opts.port, opts.model)
      break

    case 'prompt':
      await runPrompt(opts)
      break

    case 'update': {
      const { runUpdate } = await import('./update/index.js')
      const { version } = await import('./native/index.js')
      console.log('  checking for updates...')
      const result = await runUpdate(version())
      switch (result.kind) {
        case 'up_to_date': console.log('  ✓ evot is up to date.'); break
        case 'updated': console.log(`  ✓ updated ${result.from} → ${result.to}`); break
        case 'error': console.error(`  ✗ ${result.message}`); process.exit(1)
      }
      break
    }

    case 'repl':
    default: {
      const agent = createAgent(opts)
      process.on('SIGINT', () => {})

      // Preload data for the startup banner (Static renders only once)
      let preloadedSessions: Awaited<ReturnType<typeof agent.listSessions>> = []
      try { preloadedSessions = await agent.listSessions(20) } catch { /* ignore */ }

      let preloadedReleaseNotes: string[] = []
      try {
        const { fetchRecentReleaseNotes } = await import('./update/check.js')
        preloadedReleaseNotes = await fetchRecentReleaseNotes(4)
      } catch { /* ignore */ }

      const { waitUntilExit } = render(React.createElement(REPL, {
        agent,
        initialVerbose: opts.verbose,
        initialResume: opts.resume,
        preloadedSessions,
        preloadedReleaseNotes,
      }), {
        exitOnCtrlC: false,
      })
      waitUntilExit().then(() => {
        process.exit(0)
      })
      break
    }
  }
}

main()
