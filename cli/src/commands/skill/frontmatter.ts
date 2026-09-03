export interface Requires {
  env: string[]
  bins: string[]
  envHints: Record<string, string>
}

interface Node {
  value?: string
  keys: Map<string, Node>
  items: Node[]
}

function emptyNode(): Node {
  return { keys: new Map(), items: [] }
}

function unquote(value: string): string {
  const trimmed = value.trim()
  if (trimmed.length > 1 && /^["'].*["']$/.test(trimmed)) return trimmed.slice(1, -1)
  return trimmed
}

function splitFrontmatter(content: string): string[] | null {
  const lines = content.replace(/^\uFEFF/, '').split(/\r?\n/)
  const start = lines.findIndex((line) => line.trim())
  if (start < 0 || lines[start]!.trim() !== '---') return null
  const end = lines.findIndex((line, index) => index > start && line.trim() === '---')
  if (end < 0) return null
  return lines.slice(start + 1, end)
}

function indentOf(line: string): number {
  return line.length - line.trimStart().length
}

function inlineList(value: string): Node[] {
  const inner = value.trim().slice(1, -1)
  return inner
    .split(',')
    .map((item) => unquote(item))
    .filter(Boolean)
    .map((item) => ({ value: item, keys: new Map(), items: [] }))
}

function isMeaningful(line: string): boolean {
  const trimmed = line.trim()
  return Boolean(trimmed) && !trimmed.startsWith('#')
}

function assign(node: Node, key: string, rest: string): Node | null {
  const child = emptyNode()
  node.keys.set(key, child)
  if (rest.startsWith('[') && rest.endsWith(']')) {
    child.items = inlineList(rest)
    return null
  }
  if (rest) {
    child.value = unquote(rest)
    return null
  }
  return child
}

function parseBlock(lines: string[], start: number, indent: number, node: Node): number {
  let index = start
  while (index < lines.length) {
    const line = lines[index]!
    if (!isMeaningful(line)) {
      index += 1
      continue
    }
    const currentIndent = indentOf(line)
    if (currentIndent < indent) return index

    const trimmed = line.trim()
    if (trimmed.startsWith('-')) {
      const rest = trimmed.slice(1).trim()
      const item = emptyNode()
      node.items.push(item)
      const pair = rest.match(/^([A-Za-z0-9_.-]+):\s*(.*)$/)
      if (!pair) {
        item.value = unquote(rest)
        index += 1
        continue
      }
      const nested = assign(item, pair[1]!, pair[2]!.trim())
      index = nested
        ? parseBlock(lines, index + 1, currentIndent + 2, nested)
        : index + 1
      index = parseBlock(lines, index, currentIndent + 2, item)
      continue
    }

    const pair = trimmed.match(/^([A-Za-z0-9_.-]+):\s*(.*)$/)
    if (!pair) {
      index += 1
      continue
    }
    const child = assign(node, pair[1]!, pair[2]!.trim())
    index = child ? parseBlock(lines, index + 1, currentIndent + 1, child) : index + 1
  }
  return index
}

function scalars(node: Node | undefined): string[] {
  if (!node) return []
  if (node.items.length) {
    return node.items.map((item) => item.value).filter((value): value is string => Boolean(value))
  }
  return node.value ? [node.value] : []
}

function pushUnique(target: string[], values: string[]): void {
  for (const value of values) if (!target.includes(value)) target.push(value)
}

function collectEnvVars(node: Node | undefined, into: Requires): void {
  if (!node) return
  for (const item of node.items) {
    const name = item.keys.get('name')?.value
    if (!name) continue
    if (item.keys.get('required')?.value === 'false') continue
    pushUnique(into.env, [name])
    const description = item.keys.get('description')?.value
    if (description) into.envHints[name] = description
  }
}

function collectHints(node: Node | undefined, into: Requires): void {
  if (!node) return
  for (const [key, child] of node.keys) {
    if (child.value) into.envHints[key] = child.value
  }
}

export function parseRequires(content: string): Requires {
  const result: Requires = { env: [], bins: [], envHints: {} }
  const lines = splitFrontmatter(content)
  if (!lines) return result

  const root = emptyNode()
  parseBlock(lines, 0, 0, root)

  const metadata = root.keys.get('metadata')
  if (!metadata) return result

  for (const scope of [metadata.keys.get('evot'), metadata]) {
    if (!scope) continue
    const requires = scope.keys.get('requires')
    if (requires) {
      pushUnique(result.env, scalars(requires.keys.get('env')))
      pushUnique(result.bins, scalars(requires.keys.get('bins')))
    }
    collectEnvVars(scope.keys.get('envVars'), result)
    collectHints(scope.keys.get('envHints'), result)
  }
  return result
}
