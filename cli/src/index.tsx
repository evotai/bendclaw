#!/usr/bin/env bun
/**
 * bendclaw CLI — TypeScript entry point.
 * Launches the Ink-based REPL with native Rust agent backend.
 */

import React from 'react'
import { render } from 'ink'
import { Agent, version } from './native/index.js'
import { REPL } from './screens/REPL.js'

function main() {
  // Parse minimal CLI args
  const args = process.argv.slice(2)
  let model: string | undefined

  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--model' && args[i + 1]) {
      model = args[i + 1]
      i++
    }
    if (args[i] === '--version' || args[i] === '-v') {
      console.log(`bendclaw v${version()}`)
      process.exit(0)
    }
    if (args[i] === '--help' || args[i] === '-h') {
      console.log(`bendclaw v${version()} — AI coding assistant`)
      console.log()
      console.log('Usage: bendclaw-ui [options]')
      console.log()
      console.log('Options:')
      console.log('  --model <name>   Override the model')
      console.log('  --version, -v    Show version')
      console.log('  --help, -h       Show this help')
      process.exit(0)
    }
  }

  // Create native agent
  let agent: Agent
  try {
    agent = Agent.create(model)
  } catch (err: any) {
    console.error(`Failed to initialize: ${err?.message ?? err}`)
    process.exit(1)
  }

  // Launch Ink REPL
  const { waitUntilExit } = render(React.createElement(REPL, { agent }))
  waitUntilExit().then(() => {
    process.exit(0)
  })
}

main()
