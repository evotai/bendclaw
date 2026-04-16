import { Agent, version } from './native/index.js'

export interface CliOptions {
  command: 'repl' | 'serve' | 'prompt' | 'update'
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

export function parseArgs(argv: string[]): CliOptions {
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

    if (arg === 'serve' || arg === 'server') {
      opts.command = 'serve'
      continue
    }

    if (arg === 'update' || arg === '--update') {
      opts.command = 'update'
      continue
    }

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

    if (arg === '--verbose') { opts.verbose = true; continue }

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

export function parseIntArg(value: string, flag: string): number {
  const n = parseInt(value, 10)
  if (isNaN(n) || n <= 0) {
    console.error(`Invalid ${flag}: ${value} (expected positive integer)`)
    process.exit(1)
  }
  return n
}

export function printHelp() {
  console.log(`evot v${version()} — AI coding assistant`)
  console.log()
  console.log('Usage: evot [command] [options]')
  console.log()
  console.log('Commands:')
  console.log('  (default)              Interactive REPL')
  console.log('  serve                  Start HTTP server')
  console.log('  update                 Update evot to latest version')
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
  console.log('  --update               Update evot to latest version')
  console.log('  --help, -h             Show this help')
}

export function applyCliOpts(agent: Agent, opts: CliOptions): void {
  agent.setLimits(opts.maxTurns, opts.maxTokens, opts.maxDuration)
  if (opts.appendSystemPrompt) agent.appendSystemPrompt(opts.appendSystemPrompt)
  if (opts.skillsDirs.length > 0) agent.addSkillsDirs(opts.skillsDirs)
}

export function createAgent(opts: CliOptions): Agent {
  try {
    const agent = Agent.create(opts.model)
    applyCliOpts(agent, opts)
    return agent
  } catch (err: any) {
    console.error(`Failed to initialize: ${err?.message ?? err}`)
    process.exit(1)
  }
}
