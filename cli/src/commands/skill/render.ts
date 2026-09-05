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

import { getTheme } from '../../render/theme/index.js'
import { sectionHeaderLines, SECTION_MUTED } from '../../render/section.js'
import { renderCommandNotice } from '../../render/command-notice.js'
import { OFFICIAL_URL } from './source.js'

const INDENT = 2
const MEMBER_INDENT = 4
const COLUMN_GAP = 2

/** Matches the `dim: true` span the viewmodel paints, so `/skill` sits in the
 *  same visual register as every other system line. */
function muted(text: string): string {
  return chalk.hex(SECTION_MUTED)(text)
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
  /** Whether this unit belongs to the auto-managed official catalog. */
  official?: boolean
  /** Skills inside a group, with the group's own name prefix stripped. Empty for a lone skill. */
  members: string[]
}

export interface SkillListView {
  units: SkillUnitView[]
  /** Total skills across every unit. */
  total: number
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

const CUSTOM_HINT = '/skill install <source> · update [name] · remove <name>'

interface Row {
  marker: string
  label: string
  origin: string
  official: boolean
  count: string
}

function toRow(unit: SkillUnitView): Row {
  const grouped = unit.members.length > 0
  return {
    marker: grouped ? '●' : '○',
    label: unit.label,
    origin: unit.origin,
    official: unit.official === true,
    count: grouped ? String(unit.members.length) : '',
  }
}

function rowLabelWidth(row: Row): number {
  return stringWidth(row.label)
}

function renderRow(row: Row, labelWidth: number, originWidth: number): string {
  const theme = getTheme()
  const head = `${' '.repeat(INDENT)}${theme.accent.paint(row.marker)} ${theme.brandBold.paint(row.label)}`
  if (!row.origin && !row.count) return head
  const labelPad = ' '.repeat(labelWidth - rowLabelWidth(row) + COLUMN_GAP)
  if (!row.count) return `${head}${labelPad}${muted(row.origin)}`
  const originPad = ' '.repeat(originWidth - stringWidth(row.origin) + COLUMN_GAP)
  return `${head}${labelPad}${muted(row.origin)}${originPad}${muted(row.count)}`
}

/** Shared categorized inventory used by the startup banner and `/skill list`. */
export function renderSkillInventoryLines(view: SkillListView, width: number): string[] {
  if (view.units.length === 0) return []

  const rows = view.units.map(toRow)
  const official = rows.filter((row) => row.official)
  const custom = rows.filter((row) => !row.official)
  const labelWidth = Math.max(...rows.map(rowLabelWidth))
  const groupedRows = rows.filter((row) => row.count !== '')
  // Only group rows carry a count, so only they need the origin column padded.
  // Sizing it across every row let one long directory path push the counts far
  // to the right of the origins they follow.
  const originWidth = groupedRows.length
    ? Math.max(...groupedRows.map((row) => stringWidth(row.origin)))
    : 0

  const unitWord = view.units.length === 1 ? 'unit' : 'units'
  const lines = sectionHeaderLines('Skills', width, `${view.total} · ${view.units.length} ${unitWord}`)
  lines.push('')

  const appendRows = (section: Row[]): void => {
    for (const row of section) lines.push(renderRow(row, labelWidth, originWidth))
  }

  if (official.length > 0) {
    lines.push(...sectionHeaderLines('Official', width, `auto-updated · ${OFFICIAL_URL}`))
    appendRows(official)
    if (custom.length > 0) lines.push('')
  }

  if (custom.length > 0) {
    lines.push(...sectionHeaderLines('Custom', width))
    appendRows(custom)
  }

  return lines
}

/** The `/skill list` block: shared inventory plus its management hint. */
export function renderSkillList(view: SkillListView, width: number): string {
  if (view.units.length === 0) return renderNotice('no skills installed')

  const lines = ['', ...renderSkillInventoryLines(view, width)]
  lines.push('', `${' '.repeat(INDENT)}${muted(CUSTOM_HINT)}`)
  return lines.join('\n')
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
  return renderCommandNotice({ message: text })
}

export function renderRemoved(text: string): string {
  return renderCommandNotice({ state: 'success', message: text })
}

/** The in-progress status line, replaced in place as the phases advance. */
export function renderProgress(phase: string): string {
  return renderCommandNotice({ state: 'progress', label: 'skill', message: phase })
}
