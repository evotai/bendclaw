#!/usr/bin/env bun
/**
 * bendclaw CLI — TypeScript entry point.
 * Unified entry: repl (default), serve, prompt (-p).
 */

import React from 'react'
import { render } from 'ink'
import { Agent, version, startServer } from './native/index.js'
import { REPL } from './screens/REPL.js'

// ---------------------------------------------------------------------------
// Arg parsing
// ---------------------------------------------------------------------------

interface CliOptions {
  command: 'repl' | 'serve' | 'prompt'
  model?: string
  prompt?: string
  port?: number
  resume?: string
  outputFormat: 'text' | 'stream-json'
  verbose: boolean
  maxTurns: number
  maxTokens: number
  maxDuration: number
  appendSystemPrompt?: string
  skillsDirs: string[]
}

function parseArgs(argv: string[]): CliOptions {
  const opts: CliOptions = {
    command: 'repl',
    outputFormat: 'text',
    verbose: false,
    maxTurns: 512,
    maxTokens: 100_000_000,
    maxDuration: 3600,
    skillsDirs: [],
  }

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]

    // Subcommands
    if (arg === 'serve' || arg === 'server') {
      opts.command = 'serve'
      continue
    }

    // Flags with values
    if ((arg === '-p' || arg === '--prompt') && argv[i + 1]) {
      opts.command = 'prompt'
      opts.prompt = argv[++i]
      continue
    }
    if (arg === '--model' && argv[i + 1]) { opts.model = argv[++i]; continue }
    if (arg === '--port' && argv[i + 1]) { opts.port = parseInt(argv[++i], 10); continue }
    if (arg === '--resume' && argv[i + 1]) { opts.resume = argv[++i]; continue }
    if (arg === '--output-format' && argv[i + 1]) {
      opts.outputFormat = argv[++i] as 'text' | 'stream-json'
      continue
    }
    if (arg === '--max-turns' && argv[i + 1]) { opts.maxTurns = parseInt(argv[++i], 10); continue }
    if (arg === '--max-tokens' && argv[i + 1]) { opts.maxTokens = parseInt(argv[++i], 10); continue }
    if (arg === '--max-duration' && argv[i + 1]) { opts.maxDuration = parseInt(argv[++i], 10); continue }
    if (arg === '--append-system-prompt' && argv[i + 1]) { opts.appendSystemPrompt = argv[++i]; continue }
    if (arg === '--skills' && argv[i + 1]) { opts.skillsDirs.push(argv[++i]); continue }

    // Boolean flags
    if (arg === '--verbose') { opts.verbose = true; continue }

    // Info flags
    if (arg === '--version' || arg === '-v') {
      console.log(`bendclaw v${version()}`)
      process.exit(0)
    }
    if (arg === '--help' || arg === '-h') {
      printHelp()
      process.exit(0)
    }
  }

  return opts
}

function printHelp() {
  console.log(`bendclaw v${version()} — AI coding assistant`)
  console.log()
  console.log('Usage: bendclaw [command] [options]')
  console.log()
  console.log('Commands:')
  console.log('  (default)              Interactive REPL')
  console.log('  serve                  Start HTTP server')
  console.log()
  console.log('Options:')
  console.log('  -p, --prompt <text>    Run one-shot prompt')
  console.log('  --model <name>         Override the model')
  console.log('  --port <number>        Server port (default: 8082)')
  console.log('  --resume <session_id>  Resume a session')
  console.log('  --output-format <fmt>  text | stream-json (default: text)')
  console.log('  --max-turns <n>        Max turns (default: 512)')
  console.log('  --max-tokens <n>       Max tokens (default: 100000000)')
  console.log('  --max-duration <secs>  Max duration (default: 3600)')
  console.log('  --append-system-prompt <text>')
  console.log('  --skills <dir>         Skills directory (repeatable)')
  console.log('  --verbose              Verbose output')
  console.log('  --version, -v          Show version')
  console.log('  --help, -h             Show this help')
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

async function main() {
  const opts = parseArgs(process.argv.slice(2))

  switch (opts.command) {
    case 'serve':
      await startServer(opts.port, opts.model)
      break

    case 'prompt':
      await runPrompt(opts)
      break

    case 'repl':
    default:
      runRepl(opts)
      break
  }
}

async function runPrompt(opts: CliOptions) {
  let agent: Agent
  try {
    agent = Agent.create(opts.model)
  } catch (err: any) {
    console.error(`Failed to initialize: ${err?.message ?? err}`)
    process.exit(1)
  }

  const stream = await agent.query(opts.prompt!, opts.resume)
  for await (const event of stream) {
    if (opts.outputFormat === 'stream-json') {
      console.log(JSON.stringify(event))
    } else {
      printEventText(event)
    }
  }
}

function printEventText(event: any) {
  switch (event.kind) {
    case 'assistant_delta':
      if (event.payload?.delta) process.stdout.write(event.payload.delta)
      break
    case 'tool_finished':
      if (event.payload?.is_error) {
        process.stderr.write(`[error: ${event.payload.tool_name}] ${event.payload.content}\n`)
      }
      break
    case 'error':
      process.stderr.write(`error: ${event.payload?.message}\n`)
      break
    case 'run_finished':
      console.log()
      break
  }
}

function runRepl(opts: CliOptions) {
  let agent: Agent
  try {
    agent = Agent.create(opts.model)
  } catch (err: any) {
    console.error(`Failed to initialize: ${err?.message ?? err}`)
    process.exit(1)
  }

  process.on('SIGINT', () => {})

  const { waitUntilExit } = render(React.createElement(REPL, { agent }), {
    exitOnCtrlC: false,
  })
  waitUntilExit().then(() => {
    process.exit(0)
  })
}

main()
