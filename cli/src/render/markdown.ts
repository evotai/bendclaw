/**
 * Markdown rendering for terminal output.
 * Uses marked lexer + chalk + cli-highlight for proper code blocks, tables, etc.
 * Approach modeled after Claude Code's formatToken.
 */

import chalk from 'chalk'
import { marked, type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import { linkifyIssueRefs } from './linkify.js'

let highlighter: typeof import('cli-highlight') | null = null
try {
  highlighter = await import('cli-highlight')
} catch {
  // cli-highlight not available — code blocks render without syntax highlighting
}

let markedConfigured = false

export function configureMarked(): void {
  if (markedConfigured) return
  markedConfigured = true
  // Keep default tokenizer — del (~~text~~) is parsed and rendered as dim text.
  // Single ~ for "approximate" is not affected (GFM requires ~~double~~).
}

const EOL = '\n'

/**
 * Render a single marked token to an ANSI-styled string.
 */
export function formatToken(
  token: Token,
  listDepth = 0,
  orderedListNumber: number | null = null,
  parent: Token | null = null,
): string {
  switch (token.type) {
    case 'blockquote': {
      const inner = (token.tokens ?? [])
        .map(t => formatToken(t, 0, null, null))
        .join('')
      const bar = chalk.dim('▎')
      return inner
        .split(EOL)
        .map(line =>
          stripAnsi(line).trim() ? `${bar} ${chalk.italic(line)}` : line,
        )
        .join(EOL)
    }
    case 'code': {
      const text = token.text as string
      const lang = (token as Tokens.Code).lang
      let highlighted = text
      if (highlighter && lang) {
        try {
          if (highlighter.supportsLanguage(lang)) {
            highlighted = highlighter.highlight(text, { language: lang })
          }
        } catch {
          // fallback to plain text
        }
      } else if (highlighter && !lang) {
        try {
          highlighted = highlighter.highlight(text)
        } catch {
          // fallback
        }
      }
      // Borderless code block for easy copy-paste.
      return EOL + highlighted + EOL
    }
    case 'codespan':
      return chalk.blue(token.text)
    case 'del':
      return chalk.dim(
        (token.tokens ?? [])
          .map(t => formatToken(t, 0, null, parent))
          .join(''),
      )
    case 'em':
      return chalk.italic(
        (token.tokens ?? [])
          .map(t => formatToken(t, 0, null, parent))
          .join(''),
      )
    case 'strong':
      return chalk.bold(
        (token.tokens ?? [])
          .map(t => formatToken(t, 0, null, parent))
          .join(''),
      )
    case 'heading': {
      const text = (token.tokens ?? [])
        .map(t => formatToken(t, 0, null, null))
        .join('')
      return chalk.bold(text) + EOL + EOL
    }
    case 'hr':
      return '---'
    case 'link': {
      if (token.href.startsWith('mailto:')) {
        return token.href.replace(/^mailto:/, '')
      }
      const linkText = (token.tokens ?? [])
        .map(t => formatToken(t, 0, null, token))
        .join('')
      const plainText = stripAnsi(linkText)
      if (plainText && plainText !== token.href) {
        return `${linkText} (${chalk.dim(token.href)})`
      }
      return chalk.underline(token.href)
    }
    case 'list':
      return (token as Tokens.List).items
        .map((item: Token, index: number) =>
          formatToken(
            item,
            listDepth,
            (token as Tokens.List).ordered ? ((token as Tokens.List).start as number) + index : null,
            token,
          ),
        )
        .join('')
    case 'list_item':
      return (token.tokens ?? [])
        .map(
          t =>
            `${'  '.repeat(listDepth)}${formatToken(t, listDepth + 1, orderedListNumber, token)}`,
        )
        .join('')
    case 'paragraph':
      return (
        (token.tokens ?? [])
          .map(t => formatToken(t, 0, null, null))
          .join('') + EOL
      )
    case 'space':
      return EOL
    case 'br':
      return EOL
    case 'text': {
      if (parent?.type === 'link') {
        return token.text
      }
      if (parent?.type === 'list_item') {
        const bullet = orderedListNumber === null
          ? '-'
          : `${getListNumber(listDepth, orderedListNumber)}.`
        const inner = token.tokens
          ? token.tokens.map(t => formatToken(t, listDepth, orderedListNumber, token)).join('')
          : linkifyIssueRefs(token.text)
        return `${bullet} ${inner}${EOL}`
      }
      if (token.tokens) {
        return token.tokens.map(t => formatToken(t, listDepth, orderedListNumber, token)).join('')
      }
      return linkifyIssueRefs(token.text)
    }
    case 'table': {
      // Box-drawing table.
      const tableToken = token as Tokens.Table
      function getDisplayText(tokens: Token[] | undefined): string {
        return stripAnsi(
          tokens?.map(t => formatToken(t, 0, null, null)).join('') ?? '',
        )
      }
      function getDisplayWidth(tokens: Token[] | undefined): number {
        return stringWidth(getDisplayText(tokens))
      }
      // Determine column widths (using display width for CJK/emoji)
      const columnWidths = tableToken.header.map((header, index) => {
        let maxWidth = getDisplayWidth(header.tokens)
        for (const row of tableToken.rows) {
          maxWidth = Math.max(maxWidth, getDisplayWidth(row[index]?.tokens))
        }
        return Math.max(maxWidth, 3)
      })
      const numCols = columnWidths.length
      function borderLine(left: string, mid: string, cross: string, right: string): string {
        let line = left
        columnWidths.forEach((width, i) => {
          line += mid.repeat(width + 2)
          line += i < numCols - 1 ? cross : right
        })
        return line
      }
      function dataRow(cells: { tokens?: Token[] }[]): string {
        let line = '│'
        cells.forEach((cell, index) => {
          const content = cell.tokens
            ?.map(t => formatToken(t, 0, null, null))
            .join('') ?? ''
          const dw = stringWidth(stripAnsi(content))
          const width = columnWidths[index] ?? 3
          const align = tableToken.align?.[index]
          line += ' ' + padAligned(content, dw, width, align) + ' │'
        })
        return line
      }
      // Top border
      let out = borderLine('┌', '─', '┬', '┐') + EOL
      // Header row
      out += dataRow(tableToken.header) + EOL
      // Header separator
      out += borderLine('├', '─', '┼', '┤') + EOL
      // Data rows with separators between each row
      tableToken.rows.forEach((row, rowIdx) => {
        out += dataRow(row) + EOL
        if (rowIdx < tableToken.rows.length - 1) {
          out += borderLine('├', '─', '┼', '┤') + EOL
        }
      })
      // Bottom border
      out += borderLine('└', '─', '┴', '┘') + EOL
      return out + EOL
    }
    case 'escape':
      return token.text
    case 'image':
      return token.href
    case 'def':
    case 'html':
      return ''
    default:
      return ''
  }
}

/**
 * Pad content to targetWidth respecting alignment.
 * displayWidth is the visible width (caller computes via stringWidth on
 * stripAnsi'd text, so ANSI codes don't affect padding).
 */
function padAligned(
  content: string,
  displayWidth: number,
  targetWidth: number,
  align: string | null | undefined,
): string {
  const padding = Math.max(0, targetWidth - displayWidth)
  if (align === 'center') {
    const left = Math.floor(padding / 2)
    return ' '.repeat(left) + content + ' '.repeat(padding - left)
  }
  if (align === 'right') {
    return ' '.repeat(padding) + content
  }
  return content + ' '.repeat(padding)
}

// ---------------------------------------------------------------------------
// Ordered list numbering — depth-aware (number → letter → roman)
// ---------------------------------------------------------------------------

function getListNumber(listDepth: number, n: number): string {
  switch (listDepth) {
    case 0:
    case 1:
      return n.toString()
    case 2:
      return numberToLetter(n)
    case 3:
      return numberToRoman(n)
    default:
      return n.toString()
  }
}

function numberToLetter(n: number): string {
  let result = ''
  while (n > 0) {
    n--
    result = String.fromCharCode(97 + (n % 26)) + result
    n = Math.floor(n / 26)
  }
  return result
}

const ROMAN_VALUES: ReadonlyArray<[number, string]> = [
  [1000, 'm'], [900, 'cm'], [500, 'd'], [400, 'cd'],
  [100, 'c'], [90, 'xc'], [50, 'l'], [40, 'xl'],
  [10, 'x'], [9, 'ix'], [5, 'v'], [4, 'iv'], [1, 'i'],
]

function numberToRoman(n: number): string {
  let result = ''
  for (const [value, numeral] of ROMAN_VALUES) {
    while (n >= value) {
      result += numeral
      n -= value
    }
  }
  return result
}

/**
 * Render markdown text to terminal-friendly ANSI output.
 */
export function renderMarkdown(text: string): string {
  if (!text || text.trim().length === 0) return text

  configureMarked()
  try {
    const tokens = marked.lexer(text)
    return tokens
      .map(t => formatToken(t))
      .join('')
      .replace(/\n{3,}/g, '\n\n')
      .trimEnd()
  } catch {
    return text
  }
}

// ---------------------------------------------------------------------------
// Streaming markdown block splitter
// ---------------------------------------------------------------------------

export interface MarkdownSplit {
  /** Completed markdown blocks that can be committed to Static */
  completed: string
  /** Incomplete tail that stays in the dynamic zone */
  pending: string
}

/**
 * Split streaming markdown text into completed blocks and a pending tail.
 *
 * A "completed block" is a paragraph, code block, heading, list, table, etc.
 * that is fully formed and won't change with more tokens.
 *
 * Rules:
 * - A blank line (`\n\n`) is a paragraph boundary — everything before it is complete
 * - An open code fence (```) without a matching close is NOT complete
 * - The pending tail is always the text after the last safe split point
 */
export function splitMarkdownBlocks(text: string): MarkdownSplit {
  if (!text) return { completed: '', pending: '' }

  // Find the last safe split point: a blank line boundary that is NOT
  // inside an unclosed code fence.
  let splitAt = -1
  let inCodeFence = false
  let i = 0

  while (i < text.length) {
    // Detect code fence lines (``` at start of line)
    if (isAtLineStart(text, i)) {
      const fenceLen = countFence(text, i)
      if (fenceLen >= 3) {
        inCodeFence = !inCodeFence
        i += fenceLen
        // Skip to end of line (but not past the \n — let main loop handle it
        // so blank-line detection works after closing fences)
        while (i < text.length && text[i] !== '\n') i++
        continue
      }
    }

    // Detect blank line boundary (two consecutive newlines)
    if (!inCodeFence && text[i] === '\n') {
      let j = i + 1
      // Skip whitespace-only chars between newlines
      while (j < text.length && text[j] === ' ') j++
      if (j < text.length && text[j] === '\n') {
        // Found a blank line boundary — this is a safe split point
        // Include the blank line in the completed part
        splitAt = j + 1
        i = j + 1
        continue
      }
    }

    i++
  }

  // If we're inside an unclosed code fence, don't split at all
  if (inCodeFence) {
    // But if the text is very long, find the last split point BEFORE the code fence started
    // For simplicity, just return everything as pending
    return { completed: '', pending: text }
  }

  if (splitAt <= 0) {
    return { completed: '', pending: text }
  }

  return {
    completed: text.slice(0, splitAt),
    pending: text.slice(splitAt),
  }
}

function isAtLineStart(text: string, pos: number): boolean {
  return pos === 0 || text[pos - 1] === '\n'
}

function countFence(text: string, pos: number): number {
  const ch = text[pos]
  if (ch !== '`' && ch !== '~') return 0
  let count = 0
  while (pos + count < text.length && text[pos + count] === ch) count++
  return count
}
