import type { OutputLine } from '../../render/output.js'
import { line, block, plain, dim, bold, colored, type ViewBlock, type StyledLine } from './types.js'

export function buildOutputBlocks(lines: OutputLine[], initialPrevKind?: string): ViewBlock[] {
  const blocks: ViewBlock[] = []
  let prevKind: string | undefined = initialPrevKind

  for (const ol of lines) {
    switch (ol.kind) {
      case 'user':
        blocks.push(block([
          line(bold('❯ ', 'yellow'), bold(ol.text)),
        ], 1))
        break

      case 'assistant': {
        const isBlockStart = prevKind !== 'assistant'
        const dot = isBlockStart ? colored('⏺ ', 'cyan') : plain('  ')
        blocks.push(block([
          line(dot, plain(ol.text)),
        ], isBlockStart ? 1 : 0))
        break
      }

      case 'tool':
        blocks.push(buildToolBlock(ol.text))
        break

      case 'tool_result':
        blocks.push(block([line(colored(ol.text, 'gray'))]))
        break

      case 'verbose':
        blocks.push(buildVerboseBlock(ol.text))
        break

      case 'error':
        blocks.push(block([line(colored(ol.text, 'red'))]))
        break

      case 'system':
        blocks.push(block([line(dim(ol.text))]))
        break

      case 'run_summary':
        blocks.push(block([line(dim(ol.text))]))
        break

      default:
        break
    }
    prevKind = ol.kind
  }

  return blocks
}

function buildToolBlock(text: string): ViewBlock {
  const badgeMatch = text.match(/^\[([^\]]+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const isCompleted = rest.startsWith('completed')
    const isFailed = rest.startsWith('failed')
    const color: 'red' | 'green' = isFailed ? 'red' : 'green'
    const spans = [colored(`[${badge}]`, color, { bold: true })]
    if (rest) spans.push(dim(` ${rest}`))
    return block([line(...spans)], 1)
  }
  if (text.startsWith('  ')) {
    return block([line(dim(text))])
  }
  return block([line(plain(text))])
}

function buildVerboseBlock(text: string): ViewBlock {
  const badgeMatch = text.match(/^\[(\w+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const isCall = rest.startsWith('call')
    const isCompleted = rest.startsWith('completed') || rest.startsWith('·')
    const isFailed = rest.startsWith('failed')
    let color: 'red' | 'green' | 'yellow' | 'cyan' | 'magenta' = 'yellow'
    if (badge === 'LLM' || badge === 'COMPACT') {
      color = isFailed ? 'red' : 'green'
    } else if (isCompleted) {
      color = 'green'
    } else if (isFailed) {
      color = 'red'
    }
    const spans = [colored(`[${badge}]`, color, { bold: true })]
    if (rest) {
      if (isFailed) {
        spans.push(colored(` ${rest}`, 'red'))
      } else {
        spans.push(dim(` ${rest}`))
      }
    }
    return block([line(...spans)], 1)
  }
  return block([line(dim(text))])
}
