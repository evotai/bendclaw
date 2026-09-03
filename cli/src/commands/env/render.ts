import { describe, shortDate } from './format.js'

export interface VariableRow {
  key: string
  value: string
  updated_at?: string
}

const REVEAL_HINT = '  /env get KEY --reveal shows the full value'

export function renderList(rows: VariableRow[]): string {
  if (rows.length === 0) return '  no variables set'

  const sorted = [...rows].sort((a, b) => a.key.localeCompare(b.key))
  const width = Math.max(...sorted.map((row) => row.key.length))
  const shown = sorted.map((row) => ({ row, size: describe(row.value) }))
  const sizeWidth = Math.max(...shown.map((item) => item.size.length))
  const lines = shown.map(
    ({ row, size }) =>
      `  ${row.key.padEnd(width)}  ${size.padEnd(sizeWidth)}  ${shortDate(row.updated_at)}`,
  )
  return [`\n  Variables (${sorted.length})`, ...lines, '', REVEAL_HINT].join('\n')
}

export function renderGet(row: VariableRow | undefined, key: string, reveal: boolean): string {
  if (!row) return `  not set: ${key}`
  if (reveal) return `  ${row.key}=${row.value}`
  return `  ${row.key}  ${describe(row.value)}  ${shortDate(row.updated_at)}\n${REVEAL_HINT}`
}

export function renderSet(key: string, value: string): string {
  return `  set ${key}  (${describe(value)})`
}
