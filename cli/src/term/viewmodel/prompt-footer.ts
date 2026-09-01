/**
 * The status line below the prompt editor.
 *
 * Everything on this line is optional except the working directory. Terminals
 * get narrow, so the footer picks the first layout in a fixed priority order
 * that fits the available columns, dropping detail rather than wrapping or
 * overflowing.
 */

import stringWidth from 'string-width'
import { line, block, plain, dim, colored, type ViewBlock, type StyledLine, type StyledSpan } from './types.js'
import { finiteSize, spansWidth, truncateTailToWidth } from './width.js'
import { BACKGROUND_PANEL_HINT_CHORD, BACKGROUND_PANEL_SHORTCUT_HINT } from '../app/background-panel.js'

/** The subset of prompt state the footer reads. */
export interface PromptFooterVM {
  columns: number
  model: string
  provider: string
  thinkingLevel: string
  planning: boolean
  logMode: boolean
  dashboardUrl: string | null
  cwd: string
  gitBranch: string | null
  contextTokens: number
  contextWindow: number
  backgroundProcessCount: number
  /**
   * True when ↓ at the prompt opens the background panel.
   *
   * The chip names the gesture that works at this moment: ↓ is only wired up on
   * an empty composer, so advertising it mid-sentence would be a lie.
   */
  backgroundPanelDownAvailable: boolean
}

export interface PromptFooterOptions {
  /**
   * True when the composer border above already names the input mode. The
   * footer then drops its `[plan]` prefix rather than repeating the same word
   * two rows apart. It stays authoritative whenever nothing above it carries
   * the mode: overlays replace the composer, and short terminals drop the
   * border entirely.
   */
  modeShownAbove?: boolean
}

export function buildPromptFooterBlocks(
  input: PromptFooterVM,
  options: PromptFooterOptions = {},
): ViewBlock[] {
  const blocks: ViewBlock[] = []
  const chip = buildBackgroundChip(
    input.backgroundProcessCount,
    input.backgroundPanelDownAvailable,
    finiteSize(input.columns, 80),
  )
  if (chip) blocks.push(block([chip]))
  blocks.push(
    buildFooter(input, finiteSize(input.columns, 80), options.modeShownAbove ?? false),
    block([line(plain(''))]),
  )
  return blocks
}

/**
 * The background-work chip above the footer.
 *
 * The count is the actionable part, so it carries the colour while the gesture
 * stays dim: the line is a pointer to the panel, not a status report.
 *
 * The label names the background because that is what the count means: it comes
 * from `runningCount()`, which counts only `RunningBackground` shells. A bare
 * "1 shell running" left the user to guess whether it referred to the command
 * they were watching in the foreground.
 *
 * ↓ is advertised whenever it is live, because it sits one key away from the
 * cursor. With text in the composer ↓ still moves the caret, so the chip falls
 * back to naming Ctrl+T rather than a key that would do something else.
 *
 * Narrowing sheds words in order of expendability: the gesture first, then the
 * word "background", and only then characters. Truncating the long label
 * directly produced "…ound shells running", which drops the count — the one
 * part of the line that is actionable.
 */
function buildBackgroundChip(
  count: number,
  downAvailable: boolean,
  columns: number,
): StyledLine | null {
  if (count <= 0) return null
  const noun = count === 1 ? 'shell' : 'shells'
  const label = `${count} background ${noun} running`
  const shortLabel = `${count} ${noun} running`
  const chord = downAvailable ? BACKGROUND_PANEL_HINT_CHORD : BACKGROUND_PANEL_SHORTCUT_HINT
  const hint = `${chord} to manage`
  if (stringWidth(`${label} · ${hint}`) <= columns) {
    return line(colored(label, 'cyan'), dim(' · '), dim(hint))
  }
  if (stringWidth(label) <= columns) return line(colored(label, 'cyan'))
  if (stringWidth(shortLabel) <= columns) return line(colored(shortLabel, 'cyan'))
  return line(colored(truncateTailToWidth(shortLabel, columns), 'cyan'))
}

function buildFooter(input: PromptFooterVM, columns: number, modeShownAbove: boolean): ViewBlock {
  const mode = modeShownAbove
    ? ''
    : `${input.logMode ? '[log] ' : ''}${input.planning ? '[plan] ' : ''}`
  const cwd = compactCwd(input.cwd)
  const contextPercent = input.contextWindow > 0
    ? input.contextTokens / input.contextWindow * 100
    : 0

  for (const layout of FOOTER_LAYOUTS) {
    const candidate = buildFooterCandidate(input, mode, cwd, contextPercent, layout, columns)
    if (footerCandidateWidth(candidate) <= columns) return block([renderFooterCandidate(candidate, columns)])
  }

  return block([line(dim(truncateTailToWidth(`${mode}${cwd}`, columns)))])
}

type FooterContextDetail = 'full' | 'compact' | 'hidden'

interface FooterLayout {
  dashboard: boolean
  context: FooterContextDetail
  provider: boolean
  branch: boolean
  thinking: boolean
  model: boolean
  truncateCwd: boolean
}

/** Widest first: the first entry that fits wins, so detail sheds in this order. */
const FOOTER_LAYOUTS: FooterLayout[] = [
  { dashboard: true, context: 'full', provider: true, branch: true, thinking: true, model: true, truncateCwd: false },
  { dashboard: false, context: 'full', provider: true, branch: true, thinking: true, model: true, truncateCwd: false },
  { dashboard: false, context: 'compact', provider: true, branch: true, thinking: true, model: true, truncateCwd: false },
  { dashboard: false, context: 'compact', provider: false, branch: true, thinking: true, model: true, truncateCwd: false },
  { dashboard: false, context: 'compact', provider: false, branch: false, thinking: true, model: true, truncateCwd: false },
  { dashboard: false, context: 'hidden', provider: false, branch: false, thinking: true, model: true, truncateCwd: true },
  { dashboard: false, context: 'hidden', provider: false, branch: false, thinking: false, model: true, truncateCwd: true },
  { dashboard: false, context: 'hidden', provider: false, branch: false, thinking: false, model: false, truncateCwd: true },
]

interface FooterCandidate {
  left: StyledSpan[]
  dashboard: StyledSpan[] | null
}

function buildFooterCandidate(
  input: PromptFooterVM,
  mode: string,
  cwd: string,
  contextPercent: number,
  layout: FooterLayout,
  columns: number,
): FooterCandidate {
  const dashboard = layout.dashboard && input.dashboardUrl
    ? [
        { text: 'dashboard ', dim: true } satisfies StyledSpan,
        { text: input.dashboardUrl, dim: true, link: input.dashboardUrl } satisfies StyledSpan,
      ]
    : null

  const buildLeft = (location: string): StyledSpan[] => {
    const groups: StyledSpan[][] = [[dim(location)]]
    if (layout.model && input.model) {
      const identity: StyledSpan[] = [dim(input.model)]
      if (layout.provider && input.provider) identity.push(dim(`@${input.provider}`))
      if (layout.thinking && input.thinkingLevel) {
        const thinking = input.thinkingLevel === 'off' ? 'thinking off' : input.thinkingLevel
        identity.push(dim(` • ${thinking}`))
      }
      groups.push(identity)
    }
    if (layout.context !== 'hidden' && contextPercent > 0) {
      const warning = contextPercent > 90 ? ' ⚠' : ''
      const detail = layout.context === 'full'
        ? ` (${formatContextTokens(input.contextTokens)}/${formatContextTokens(input.contextWindow)})`
        : ''
      const text = `context: ${contextPercent.toFixed(1)}%${detail}${warning}`
      groups.push([
        contextPercent > 90
          ? colored(text, 'red')
          : contextPercent > 70
            ? colored(text, 'yellow')
            : dim(text),
      ])
    }
    return groups.flatMap((group, index) => index === 0 ? group : [dim(' │ '), ...group])
  }

  const branch = layout.branch && input.gitBranch ? ` (${input.gitBranch})` : ''
  const fullLocation = `${mode}${cwd}${branch}`
  let left = buildLeft(fullLocation)
  let candidate = { left, dashboard }
  if (!layout.truncateCwd || footerCandidateWidth(candidate) <= columns) return candidate

  const fixedWidth = footerCandidateWidth(candidate) - stringWidth(fullLocation)
  const availableLocationWidth = Math.max(1, columns - fixedWidth)
  left = buildLeft(truncateTailToWidth(`${mode}${cwd}`, availableLocationWidth))
  candidate = { left, dashboard }
  return candidate
}

function footerCandidateWidth(candidate: FooterCandidate): number {
  const left = spansWidth(candidate.left)
  return candidate.dashboard ? left + 2 + spansWidth(candidate.dashboard) : left
}

function renderFooterCandidate(candidate: FooterCandidate, columns: number): StyledLine {
  if (!candidate.dashboard) return line(...candidate.left)
  const padding = columns - spansWidth(candidate.left) - spansWidth(candidate.dashboard)
  return line(...candidate.left, plain(' '.repeat(Math.max(2, padding))), ...candidate.dashboard)
}

function compactCwd(cwd: string): string {
  const home = process.env.HOME || process.env.USERPROFILE || ''
  return home && cwd.startsWith(home) ? `~${cwd.slice(home.length)}` : cwd
}

function formatContextTokens(count: number): string {
  if (count < 1000) return `${count}`
  if (count < 1000000) {
    const k = count / 1000
    return `${Number.isInteger(k) ? k : k.toFixed(1)}k`
  }
  const m = count / 1000000
  return `${Number.isInteger(m) ? m : m.toFixed(1)}M`
}
