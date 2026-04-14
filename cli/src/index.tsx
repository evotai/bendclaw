#!/usr/bin/env bun
/**
 * evot CLI — TypeScript entry point.
 * Unified entry: repl (default), serve, prompt (-p).
 */

import React from 'react'
import { render } from 'ink'
import { Agent, version, startServer } from './native/index.js'
import { REPL } from './screens/REPL.js'
import { installTerminalRestore, restoreTerminalNow } from './utils/terminalRestore.js'

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
    verbose: true,
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
    if (arg === '--port' && argv[i + 1]) { opts.port = parseIntArg(argv[++i], '--port'); continue }
    if (arg === '--resume' && argv[i + 1]) { opts.resume = argv[++i]; continue }
    if (arg === '--output-format' && argv[i + 1]) {
      const fmt = argv[++i]
      if (fmt !== 'text' && fmt !== 'stream-json') {
        console.error(`Invalid --output-format: ${fmt} (expected text or stream-json)`)
        process.exit(1)
      }
      opts.outputFormat = fmt
      continue
    }
    if (arg === '--max-turns' && argv[i + 1]) { opts.maxTurns = parseIntArg(argv[++i], '--max-turns'); continue }
    if (arg === '--max-tokens' && argv[i + 1]) { opts.maxTokens = parseIntArg(argv[++i], '--max-tokens'); continue }
    if (arg === '--max-duration' && argv[i + 1]) { opts.maxDuration = parseIntArg(argv[++i], '--max-duration'); continue }
    if (arg === '--append-system-prompt' && argv[i + 1]) { opts.appendSystemPrompt = argv[++i]; continue }
    if (arg === '--skills' && argv[i + 1]) { opts.skillsDirs.push(argv[++i]); continue }

    // Boolean flags
    if (arg === '--verbose') { opts.verbose = true; continue }
    if (arg === '--no-verbose') { opts.verbose = false; continue }

    // Info flags
    if (arg === '--version' || arg === '-v') {
      console.log(`evot v${version()}`)
      process.exit(0)
    }
    if (arg === '--help' || arg === '-h') {
      printHelp()
      process.exit(0)
    }
  }

  return opts
}

function parseIntArg(value: string, flag: string): number {
  const n = parseInt(value, 10)
  if (isNaN(n) || n <= 0) {
    console.error(`Invalid ${flag}: ${value} (expected positive integer)`)
    process.exit(1)
  }
  return n
}

function printHelp() {
  console.log(`evot v${version()} — AI coding assistant`)
  console.log()
  console.log('Usage: evot [command] [options]')
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
  console.log('  --verbose              Verbose output (default: on)')
  console.log('  --no-verbose           Disable verbose output')
  console.log('  --version, -v          Show version')
  console.log('  --help, -h             Show this help')
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

async function main() {
  installTerminalRestore()
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

function applyCliOpts(agent: Agent, opts: CliOptions): void {
  agent.setLimits(opts.maxTurns, opts.maxTokens, opts.maxDuration)
  if (opts.appendSystemPrompt) agent.appendSystemPrompt(opts.appendSystemPrompt)
  if (opts.skillsDirs.length > 0) agent.addSkillsDirs(opts.skillsDirs)
}

async function runPrompt(opts: CliOptions) {
  if (!opts.prompt) {
    console.error('No prompt provided. Use -p <text>')
    process.exit(1)
  }

  let agent: Agent
  try {
    agent = Agent.create(opts.model)
    applyCliOpts(agent, opts)
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
  process.exit(0)
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
    applyCliOpts(agent, opts)
  } catch (err: any) {
    console.error(`Failed to initialize: ${err?.message ?? err}`)
    process.exit(1)
  }

  process.on('SIGINT', () => {})

  const { waitUntilExit } = render(React.createElement(REPL, {
    agent,
    initialVerbose: opts.verbose,
    initialResume: opts.resume,
  }), {
    exitOnCtrlC: false,
  })
  waitUntilExit().then(() => {
    restoreTerminalNow()
    process.exit(0)
  })
}

main()
