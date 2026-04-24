import { formatSpinnerLine, type SpinnerState } from '../spinner.js'
import { line, block, plain, dim, colored, type ViewBlock, type StyledLine } from './types.js'
import { renderMarkdown } from '../../render/markdown.js'

const MAX_PROGRESS_LINES = 5
const MAX_PROGRESS_LINE_WIDTH = 120

export interface ActiveResponseInput {
  isLoading: boolean
  pendingText: string
  toolProgress: string
  spinner: SpinnerState
  termRows: number
  expanded?: boolean
  assistantCommitted?: boolean
}

export function buildActiveResponseBlocks(input: ActiveResponseInput): ViewBlock[] {
  if (!input.isLoading) return []

  const blocks: ViewBlock[] = []

  if (input.pendingText) {
    // Show the trailing fragment of in-progress streaming text.
    // Completed markdown blocks are committed to scroll area by the stream machine;
    // this shows the current incomplete block being streamed.
    // Match scroll area style: first line gets ⏺ dot + margin if this is the
    // start of an assistant block (nothing committed yet).
    const rendered = renderMarkdown(input.pendingText)
    const lines = (rendered || input.pendingText).split('\n')
    // Reserve space for spinner + prompt; show as many trailing lines as fit
    const maxLines = Math.max(1, input.termRows - 10)
    const visible = lines.slice(-maxLines)
    const isBlockStart = !input.assistantCommitted
    // The ⏺ dot marks the start of an assistant block. It should only appear
    // on the block's first line (lines[0]). If the block has grown beyond the
    // visible window, the first line is no longer visible — don't put ⏺ on
    // visible[0] because that would make a middle line suddenly gain a dot.
    const blockStartVisible = isBlockStart && lines.length <= maxLines
    const styledLines: StyledLine[] = visible.map((l, i) =>
      i === 0 && blockStartVisible
        ? line(colored('⏺ ', 'cyan'), plain(l))
        : line(plain(`  ${l}`))
    )
    if (styledLines.length > 0) {
      blocks.push(block(styledLines, blockStartVisible ? 1 : 0))
    }
  }

  if (input.toolProgress && !input.expanded) {
    const progLines = input.toolProgress.split('\n')
    const extraLines = Math.max(0, progLines.length - MAX_PROGRESS_LINES)
    const tail = progLines
      .slice(-MAX_PROGRESS_LINES)
      .map(l => l.length > MAX_PROGRESS_LINE_WIDTH ? l.slice(0, MAX_PROGRESS_LINE_WIDTH - 1) + '…' : l)
    const styledLines: StyledLine[] = tail.map(l => line(dim(`  ${l}`)))
    if (extraLines > 0) {
      styledLines.push(line(dim(`  +${extraLines} lines  (ctrl+o to expand)`)))
    }
    blocks.push(block(styledLines, 1))
  }

  const spinnerText = formatSpinnerLine(input.spinner, Date.now())
  blocks.push(block(
    [line(plain(spinnerText))],
    1,
  ))

  return blocks
}
