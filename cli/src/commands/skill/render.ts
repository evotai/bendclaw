/**
 * Presentation for `/skill`.
 *
 * Kept apart from the filesystem walk and the install machinery so every layout
 * decision is a pure function of a model plus a terminal width. The grid math is
 * the part worth testing, and it should not need a real skills directory to run.
 *
 * Output here is already styled, so the lines it produces must be committed with
 * `preStyled: true` — the REPL's default `system` treatment paints whole lines
 * one flat gray, which is what made this command unreadable.
 */

import chalk from 'chalk'
import { homedir } from 'os'
import stringWidth from 'string-width'

import { getTheme } from '../../render/theme.js'

const INDENT = 2
const MEMBER_INDENT = 4
const COLUMN_GAP = 2
const MEMBER_GAP = 3
/** Beyond this, member columns get too narrow to scan. */
const MAX_MEMBER_COLUMNS = 8

/** Matches the `dim: true` span the viewmodel paints, so `/skill` sits in the
 *  same visual register as every other system line. */
function muted(text: string): string {
  return chalk.hex('#777777')(text)
}

export function tildify(dir: string): string {
  const home = homedir()
  if (dir === home) return '~'
  return dir.startsWith(`${home}/`) ? `~${dir.slice(home.length)}` : dir
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

export interface SkillUnitView {
  /** Unit label: the group directory name, or a lone skill's own name. */
  name: string
  /** Display form of `name`. Directory groups carry a trailing `/`. */
  label: string
  /** Short source label: `@40b5130`, `acme/pack@1a2b3c4`, or `~/.evotai/skills`. */
  origin: string
  /** Skills inside a group, with the group's own name prefix stripped. Empty for a lone skill. */
  members: string[]
}

export interface SkillListView {
  units: SkillUnitView[]
  /** Total skills across every unit. */
  total: number
}

/** Drop the redundant `lark-` from `lark-im` when it sits under `● lark/`. */
export function shortMemberName(unit: string, member: string): string {
  const prefix = `${unit}-`
  return member.startsWith(prefix) && member.length > prefix.length
    ? member.slice(prefix.length)
    : member
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

/**
 * Split items into columns, filling top to bottom.
 *
 * Column-major, like `ls`. Filling left to right instead would put a long name
 * in every column it touched, so one 24-character skill widened the whole first
 * column and every short name after it sat behind a gap. Down-filling confines
 * that cost to the one column the long name lands in.
 */
function columnsOf(items: string[], maxColumns: number): string[][] {
  const rows = Math.ceil(items.length / maxColumns)
  const columns: string[][] = []
  for (let start = 0; start < items.length; start += rows) {
    columns.push(items.slice(start, start + rows))
  }
  return columns
}

function layoutColumns(columns: string[][], indent: number, gap: number): string[] {
  const widths = columns.map((column) => Math.max(...column.map((item) => stringWidth(item))))
  const rowCount = Math.max(...columns.map((column) => column.length))
  const rows: string[] = []
  for (let row = 0; row < rowCount; row++) {
    let text = ' '.repeat(indent)
    // Trailing empty cells are never padded, so a short last row adds no
    // invisible width the caller would have to account for.
    const last = columns.reduce((found, column, index) => (column[row] ? index : found), -1)
    for (let column = 0; column <= last; column++) {
      const item = columns[column]?.[row] ?? ''
      text += item
      if (column < last) text += ' '.repeat((widths[column] ?? 0) - stringWidth(item) + gap)
    }
    rows.push(text)
  }
  return rows
}

function fits(columns: string[][], available: number, gap: number): boolean {
  const total = columns.reduce(
    (sum, column) => sum + Math.max(...column.map((item) => stringWidth(item))),
    0,
  )
  return total + gap * (columns.length - 1) <= available
}

/**
 * Lay items out in as many columns as the width allows, reading top to bottom.
 *
 * Each column is sized from the items that actually land in it, so one long
 * name does not widen the whole grid the way a single uniform cell width would.
 */
export function gridLines(
  items: string[],
  width: number,
  indent = MEMBER_INDENT,
  gap = MEMBER_GAP,
): string[] {
  if (items.length === 0) return []
  const available = Math.max(1, width - indent)
  const max = Math.min(items.length, MAX_MEMBER_COLUMNS)
  for (let count = max; count > 1; count--) {
    const columns = columnsOf(items, count)
    // A smaller `count` can produce the same row count, and therefore the same
    // columns, as one already rejected; measuring again is cheap and keeps the
    // loop a plain search over candidates.
    if (fits(columns, available, gap)) return layoutColumns(columns, indent, gap)
  }
  return layoutColumns([items], indent, gap)
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

const HINT = '/skill install <source> · update [name] · remove <name>'

interface Row {
  marker: string
  label: string
  origin: string
  count: string
  members: string[]
}

function toRow(unit: SkillUnitView): Row {
  const grouped = unit.members.length > 0
  return {
    marker: grouped ? '●' : '○',
    label: unit.label,
    origin: unit.origin,
    count: grouped ? String(unit.members.length) : '',
    members: unit.members.map((member) => shortMemberName(unit.name, member)),
  }
}

function renderRow(row: Row, labelWidth: number, originWidth: number): string {
  const theme = getTheme()
  const head = `${' '.repeat(INDENT)}${theme.accent.paint(row.marker)} ${theme.brandBold.paint(row.label)}`
  if (!row.origin && !row.count) return head
  const labelPad = ' '.repeat(labelWidth - stringWidth(row.label) + COLUMN_GAP)
  if (!row.count) return `${head}${labelPad}${muted(row.origin)}`
  const originPad = ' '.repeat(originWidth - stringWidth(row.origin) + COLUMN_GAP)
  return `${head}${labelPad}${muted(row.origin)}${originPad}${muted(row.count)}`
}

/**
 * The `/skill list` block.
 *
 * Groups come first with their members spread across columns; lone skills
 * follow as a compact table. Both share one label and one origin column so the
 * whole block reads as a single list rather than two unrelated shapes.
 */
export function renderSkillList(view: SkillListView, width: number): string {
  // Routed through renderNotice, not returned bare: the caller commits this as
  // pre-styled, so unstyled text would render undimmed while every other
  // one-line result is gray.
  if (view.units.length === 0) return renderNotice('no skills installed')

  const rows = view.units.map(toRow)
  const groups = rows.filter((row) => row.members.length > 0)
  const singles = rows.filter((row) => row.members.length === 0)
  const labelWidth = Math.max(...rows.map((row) => stringWidth(row.label)))
  // Only group rows carry a count, so only they need the origin column padded.
  // Sizing it across every row let one long directory path push the counts far
  // to the right of the origins they follow.
  const originWidth = groups.length
    ? Math.max(...groups.map((row) => stringWidth(row.origin)))
    : 0

  const theme = getTheme()
  const unitWord = view.units.length === 1 ? 'unit' : 'units'
  const lines: string[] = [
    '',
    `${' '.repeat(INDENT)}${theme.brandBold.paint('Skills')}  ${muted(`${view.total} · ${view.units.length} ${unitWord}`)}`,
    '',
  ]
  for (const row of groups) {
    lines.push(renderRow(row, labelWidth, originWidth))
    lines.push(...gridLines(row.members, width).map((line) => muted(line)))
    lines.push('')
  }
  for (const row of singles) lines.push(renderRow(row, labelWidth, originWidth))
  if (singles.length > 0) lines.push('')
  lines.push(`${' '.repeat(INDENT)}${muted(HINT)}`)
  return lines.join('\n')
}

/**
 * The startup banner's one-line-per-unit summary.
 *
 * The banner is a glance, not an inventory, so members collapse into a count:
 * spelling out all 27 lark skills cost three wrapped lines and told the user
 * nothing they could act on. Labels follow `/skill list` exactly, so `lark/`
 * means the same thing in both places.
 *
 * `mutedHex` exists because the banner's own secondary text is a different gray
 * from the REPL's. Left to the default, the counts would sit a shade off from
 * the `[Context]` values directly above them.
 */
export function skillSummaryParts(view: SkillListView, mutedHex?: string): string[] {
  const theme = getTheme()
  const secondary = mutedHex ? (text: string) => chalk.hex(mutedHex)(text) : muted
  return view.units.map((unit) =>
    unit.members.length > 0
      ? `${theme.brand.paint(unit.label)} ${secondary(String(unit.members.length))}`
      : theme.brand.paint(unit.label),
  )
}

/** Joined form of {@link skillSummaryParts}, ready to wrap. */
export function renderSkillSummary(view: SkillListView, mutedHex?: string): string {
  const separator = mutedHex ? chalk.hex(mutedHex)(' · ') : muted(' · ')
  return skillSummaryParts(view, mutedHex).join(separator)
}

// ---------------------------------------------------------------------------
// Install / update / remove
// ---------------------------------------------------------------------------

export type UnitOutcome = 'new' | 'updated' | 'unchanged' | 'failed' | 'skipped'

export interface UnitNote {
  /** `warn` for something the user must act on, `info` for what we did for them. */
  kind: 'warn' | 'info'
  text: string
}

export interface UnitResult {
  name: string
  /** Skills the unit contains. `> 1` marks it as a group. */
  skills: number
  outcome: UnitOutcome
  /** Version transition, reason, or error message for the trailing column. */
  detail: string
  /** Requirement gaps and superseded removals, shown under the unit. */
  notes: UnitNote[]
}

export interface OperationView {
  title: string
  /** Only set when one source explains the whole operation (install). */
  source?: string
  units: UnitResult[]
  /** Skills present in the managed root afterwards, or undefined to omit. */
  total?: number
}

/** Either a one-line refusal/notice, or a full operation report. */
export type SkillOutcome = { notice: string } | { view: OperationView }

function marker(outcome: UnitOutcome): string {
  switch (outcome) {
    case 'new': return chalk.green('✓')
    case 'updated': return chalk.green('↑')
    case 'unchanged': return muted('=')
    case 'failed': return chalk.red('✗')
    case 'skipped': return muted('-')
  }
}

function unitLabel(unit: UnitResult): string {
  return unit.skills > 1 ? `${unit.name}/` : unit.name
}

function unitSize(unit: UnitResult): string {
  return unit.skills > 1 ? `${unit.skills} skills` : ''
}

export function renderOperation(view: OperationView): string {
  const theme = getTheme()
  const head = view.source
    ? `${' '.repeat(INDENT)}${theme.brandBold.paint(view.title)}  ${muted(view.source)}`
    : `${' '.repeat(INDENT)}${theme.brandBold.paint(view.title)}`
  if (view.units.length === 0) return head

  const labelWidth = Math.max(...view.units.map((unit) => stringWidth(unitLabel(unit))))
  const sizeWidth = Math.max(...view.units.map((unit) => stringWidth(unitSize(unit))))

  const lines: string[] = [head, '']
  for (const unit of view.units) {
    const label = unitLabel(unit)
    const size = unitSize(unit)
    let text = `${' '.repeat(INDENT)}${marker(unit.outcome)} ${theme.brandBold.paint(label)}`
    if (size || unit.detail) {
      text += ' '.repeat(labelWidth - stringWidth(label) + COLUMN_GAP)
      text += muted(size)
    }
    if (unit.detail) {
      text += ' '.repeat(sizeWidth - stringWidth(size) + COLUMN_GAP)
      text += unit.outcome === 'failed' ? chalk.red(unit.detail) : muted(unit.detail)
    }
    lines.push(text)
    for (const note of unit.notes) {
      const glyph = note.kind === 'warn' ? chalk.yellow('!') : muted('·')
      lines.push(`${' '.repeat(MEMBER_INDENT)}${glyph} ${muted(note.text)}`)
    }
  }
  if (view.total !== undefined) {
    lines.push('', `${' '.repeat(INDENT)}${muted(`${view.total} skills installed`)}`)
  }
  return lines.join('\n')
}

/** A single-line result, for `/skill remove` and every refusal path. */
export function renderNotice(text: string): string {
  return `${' '.repeat(INDENT)}${muted(text)}`
}

export function renderRemoved(text: string): string {
  return `${' '.repeat(INDENT)}${chalk.green('✓')} ${muted(text)}`
}

/** The in-progress status line, replaced in place as the phases advance. */
export function renderProgress(phase: string): string {
  return `${' '.repeat(INDENT)}${muted('⋯')} ${getTheme().brandBold.paint('skill')}  ${muted(phase)}`
}
