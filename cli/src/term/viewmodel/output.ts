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
        // Empty-text assistant lines are block-spacing separators inserted by
        // the stream machine.  Don't give them the ⏺ dot and don't let them
        // flip prevKind so the next non-empty line still counts as block start.
        if (!ol.text) {
          blocks.push(block([line(plain(''))]))
          break   // intentionally skip prevKind update
        }
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
    const statusMatch = rest.match(/^([●✓✗])\s*(.*)$/)
    const spans = [colored(`[${badge}]`, 'cyan', { bold: true })]

    if (statusMatch) {
      spans.push(colored(` ${statusMatch[1]}`, 'cyan', { bold: true }))
      const tail = statusMatch[2] ?? ''
      if (tail) spans.push(dim(tail.startsWith(' ') ? tail : ` ${tail}`))
    } else if (rest) {
      spans.push(dim(` ${rest}`))
    }

    return block([line(...spans)], 1)
  }
  if (text.startsWith('  ')) {
    const trimmed = text.trimStart()
    if (/^[{}\[\],]/.test(trimmed) || /^"[^"\\]*(?:\\.[^"\\]*)*"\s*:/.test(trimmed)) {
      return block([line(plain(text))])
    }
    return block([line(dim(text))])
  }
  return block([line(plain(text))])
}

function buildVerboseBlock(text: string): ViewBlock {
  const badgeMatch = text.match(/^\[(\w+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const statusMatch = rest.match(/^([●✓✗])\s*(.*)$/)
    const color = verboseStatusColor()
    const spans = [colored(`[${badge}]`, color, { bold: true })]
    if (statusMatch) {
      spans.push(colored(` ${statusMatch[1]}`, color, { bold: true }))
      const tail = statusMatch[2] ?? ''
      if (tail) spans.push(dim(` ${tail}`))
    } else if (rest) {
      spans.push(dim(` ${rest}`))
    }
    return block([line(...spans)], 1)
  }
  return block([line(dim(text))])
}

function verboseStatusColor(): 'cyan' {
  return 'cyan'
}
