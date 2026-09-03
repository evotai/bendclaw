export interface ParsedEntry {
  key: string
  value: string
}

export interface ParsedEnvFile {
  entries: ParsedEntry[]
  skipped: number
}

const KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*$/

function unquote(value: string): string {
  const text = value.trim()
  if (text.length >= 2 && text[0] === text[text.length - 1] && (text[0] === '"' || text[0] === "'")) {
    return text.slice(1, -1)
  }
  return text
}

/**
 * Parse KEY=VALUE lines from a dotenv-style file. Comments, blanks, malformed
 * lines and invalid keys are skipped rather than aborting the import, so one bad
 * line does not block the rest. A leading `export ` is tolerated because users
 * paste straight from shell profiles.
 */
export function parseEnvFile(content: string): ParsedEnvFile {
  const seen = new Map<string, string>()
  let skipped = 0

  for (const raw of content.split('\n')) {
    const line = raw.trim()
    if (!line || line.startsWith('#')) continue

    const body = line.startsWith('export ') ? line.slice(7).trim() : line
    const eq = body.indexOf('=')
    if (eq <= 0) {
      skipped += 1
      continue
    }

    const key = body.slice(0, eq).trim()
    if (!KEY_RE.test(key)) {
      skipped += 1
      continue
    }
    seen.set(key, unquote(body.slice(eq + 1)))
  }

  return {
    entries: [...seen.entries()].map(([key, value]) => ({ key, value })),
    skipped,
  }
}

export function isValidKey(key: string): boolean {
  return KEY_RE.test(key)
}
