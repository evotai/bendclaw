import { formatSpinnerLine, type SpinnerState } from '../spinner.js'
import { line, block, plain, dim, type ViewBlock, type StyledLine } from './types.js'
import { renderMarkdownCached } from '../../render/markdown.js'

const MAX_PROGRESS_LINES = 5
const MAX_PROGRESS_LINE_WIDTH = 120
/** Lines reserved for spinner + prompt + padding */
const RESERVED_LINES = 8

export interface ActiveResponseInput {
  isLoading: boolean
  pendingText: string
  toolProgress: string
  spinner: SpinnerState
  termRows: number
}

export function buildActiveResponseBlocks(input: ActiveResponseInput): ViewBlock[] {
  if (!input.isLoading) return []

  const blocks: ViewBlock[] = []

  if (input.pendingText) {
    // Render the full pending markdown (tables, code blocks, lists, etc.)
    // and show the last N lines so the user sees content as it streams in.
    const rendered = renderMarkdownCached(input.pendingText).replace(/\n+$/, '')
    const allLines = rendered.split('\n')
    const maxHeight = Math.max(input.termRows - RESERVED_LINES, 4)
    const visible = allLines.length <= maxHeight ? allLines : allLines.slice(-maxHeight)
    const styledLines: StyledLine[] = visible
      .filter(l => l.trim())
      .map(l => line(plain(`  ${l}`)))
    if (styledLines.length > 0) {
      blocks.push(block(styledLines))
    }
  }

  if (input.toolProgress) {
    const progLines = input.toolProgress.split('\n')
    const tail = progLines
      .slice(-MAX_PROGRESS_LINES)
      .map(l => l.length > MAX_PROGRESS_LINE_WIDTH ? l.slice(0, MAX_PROGRESS_LINE_WIDTH - 1) + '…' : l)
    while (tail.length < MAX_PROGRESS_LINES) tail.unshift('')
    const styledLines: StyledLine[] = tail.map(l => line(dim(`  ${l}`)))
    blocks.push(block(styledLines, 1))
  }

  const spinnerText = formatSpinnerLine(input.spinner, Date.now())
  blocks.push(block(
    [line(plain(spinnerText))],
    input.toolProgress ? 0 : 1,
  ))

  return blocks
}
