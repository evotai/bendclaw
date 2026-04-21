import { formatSpinnerLine, type SpinnerState } from '../spinner.js'
import { line, block, plain, dim, type ViewBlock, type StyledLine } from './types.js'

const MAX_PROGRESS_LINES = 5
const MAX_PROGRESS_LINE_WIDTH = 120

export interface ActiveResponseInput {
  isLoading: boolean
  pendingText: string
  toolProgress: string
  spinner: SpinnerState
  termRows: number
  expanded?: boolean
}

export function buildActiveResponseBlocks(input: ActiveResponseInput): ViewBlock[] {
  if (!input.isLoading) return []

  const blocks: ViewBlock[] = []

  if (input.pendingText) {
    // Show the trailing fragment of in-progress streaming text.
    // Completed markdown blocks are committed to scroll area by the stream machine;
    // this shows the current incomplete block being streamed.
    const lines = input.pendingText.split('\n')
    // Reserve space for spinner + prompt; show as many trailing lines as fit
    const maxLines = Math.max(1, input.termRows - 10)
    const visible = lines.slice(-maxLines)
    const styledLines: StyledLine[] = visible.map(l => line(plain(`  ${l}`)))
    if (styledLines.length > 0) {
      blocks.push(block(styledLines))
    }
  }

  if (input.toolProgress && !input.expanded) {
    const progLines = input.toolProgress.split('\n')
    const tail = progLines
      .slice(-MAX_PROGRESS_LINES)
      .map(l => l.length > MAX_PROGRESS_LINE_WIDTH ? l.slice(0, MAX_PROGRESS_LINE_WIDTH - 1) + '…' : l)
    while (tail.length < MAX_PROGRESS_LINES) tail.unshift('')
    const styledLines: StyledLine[] = tail.map(l => line(dim(`  ${l}`)))
    // Show extra line count
    const extraLines = Math.max(0, progLines.length - MAX_PROGRESS_LINES)
    if (extraLines > 0) {
      styledLines.push(line(dim(`  +${extraLines} lines`)))
    }
    blocks.push(block(styledLines, 1))
  }

  const spinnerText = formatSpinnerLine(input.spinner, Date.now())
  blocks.push(block(
    [line(plain(spinnerText))],
    input.toolProgress ? 0 : 1,
  ))

  return blocks
}
