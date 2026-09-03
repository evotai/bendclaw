import { isValidKey, parseEnvFile } from './parse.js'
import { renderGet, renderList, renderSet, type VariableRow } from './render.js'

export interface EnvPort {
  list: () => VariableRow[]
  set: (key: string, value: string) => Promise<void>
  del: (key: string) => Promise<boolean>
  readFile: (path: string) => Promise<string>
}

export const USAGE =
  '  Usage: /env [list | get KEY [--reveal] | set KEY=VALUE | del KEY | load FILE]'

function splitAssignment(text: string): { key: string; value: string } | null {
  const eq = text.indexOf('=')
  if (eq <= 0) return null
  return { key: text.slice(0, eq).trim(), value: text.slice(eq + 1) }
}

async function handleSet(port: EnvPort, rest: string): Promise<string> {
  const pair = splitAssignment(rest)
  if (!pair) return '  Usage: /env set KEY=VALUE'
  if (!isValidKey(pair.key)) {
    return `  invalid key: ${pair.key}   (letters, digits and _ only, not starting with a digit)`
  }
  await port.set(pair.key, pair.value)
  return renderSet(pair.key, pair.value)
}

function handleGet(port: EnvPort, rest: string): string {
  const parts = rest.split(/\s+/).filter(Boolean)
  const key = parts.find((part) => part !== '--reveal')
  if (!key) return '  Usage: /env get KEY [--reveal]'
  // A `--reveal` that reaches here had no matching key (the REPL intercepts the
  // ones it can show), so the masked view is the honest answer either way.
  return renderGet(
    port.list().find((row) => row.key === key),
    key,
  )
}

/**
 * The key a `/env` invocation wants revealed, or null if it wants anything else.
 *
 * Split out because a reveal is committed differently from every other `/env`
 * output: it goes to the terminal but not the screen log, and it is erased after
 * a delay. The parsing stays here beside the routing it mirrors, so the two
 * cannot drift; the timer belongs to the REPL, which owns the frame.
 */
export function parseRevealTarget(args: string): string | null {
  const input = args.trim()
  const space = input.search(/\s/)
  if (space === -1) return null
  if (input.slice(0, space) !== 'get') return null
  const parts = input.slice(space + 1).split(/\s+/).filter(Boolean)
  if (!parts.includes('--reveal')) return null
  return parts.find((part) => part !== '--reveal') ?? null
}

async function handleDel(port: EnvPort, rest: string): Promise<string> {
  const key = rest.trim()
  if (!key) return '  Usage: /env del KEY'
  const existed = await port.del(key)
  return existed ? `  deleted ${key}` : `  not set: ${key}`
}

async function handleLoad(port: EnvPort, rest: string): Promise<string> {
  const path = rest.trim()
  if (!path) return '  Usage: /env load FILE'

  let content: string
  try {
    content = await port.readFile(path)
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    return `  cannot read ${path}: ${message}`
  }

  const { entries, skipped } = parseEnvFile(content)
  if (entries.length === 0) {
    return skipped > 0
      ? `  no usable KEY=VALUE lines in ${path} (${skipped} skipped)`
      : `  no KEY=VALUE lines in ${path}`
  }

  for (const entry of entries) await port.set(entry.key, entry.value)

  // Names only. Values came from a file the user already has; echoing them here
  // would copy secrets into the transcript and the screen log.
  const names = entries.map((entry) => entry.key).join(', ')
  const tail = skipped > 0 ? `  (${skipped} line(s) skipped)` : ''
  return `  loaded ${entries.length} variable(s) from ${path}${tail}\n  ${names}`
}

/** Route one `/env` invocation. Returns the text to commit. */
export async function runEnvCommand(port: EnvPort, args: string): Promise<string> {
  const input = args.trim()
  if (!input || input === 'list') return renderList(port.list())

  const space = input.search(/\s/)
  const sub = space === -1 ? input : input.slice(0, space)
  const rest = space === -1 ? '' : input.slice(space + 1)

  switch (sub) {
    case 'get':
      return handleGet(port, rest)
    case 'set':
      return handleSet(port, rest)
    case 'del':
      return handleDel(port, rest)
    case 'load':
      return handleLoad(port, rest)
    default:
      return USAGE
  }
}
