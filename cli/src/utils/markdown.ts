/**
 * Markdown rendering for terminal output.
 * Uses marked lexer + chalk + cli-highlight for proper code blocks, tables, etc.
 * Approach modeled after Claude Code's formatToken.
 */

import chalk from 'chalk'
import { marked, type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import { linkifyIssueRefs } from './linkify.js'

let highlighter: typeof import('cli-highlight') | null = null
try {
  highlighter = await import('cli-highlight')
} catch {
  // cli-highlight not available — code blocks render without syntax highlighting
}

let markedConfigured = false

function configureMarked(): void {
  if (markedConfigured) return
  markedConfigured = true
  // Disable strikethrough — model often uses ~ for "approximate"
  marked.use({
    tokenizer: {
      del() {
        return undefined as any
      },
    },
  })
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
      const bar = chalk.dim('│')
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
      return highlighted + EOL
    }
    case 'codespan':
      return chalk.cyan(token.text)
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
      if (token.depth === 1) {
        return chalk.bold.italic.underline(text) + EOL + EOL
      }
      return chalk.bold(text) + EOL + EOL
    }
    case 'hr':
      return '---'
    case 'link': {
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
      if (parent?.type === 'list_item') {
        const bullet = orderedListNumber === null ? '-' : `${orderedListNumber}.`
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
      const tableToken = token as Tokens.Table
      function getDisplayText(tokens: Token[] | undefined): string {
        return stripAnsi(
          tokens?.map(t => formatToken(t, 0, null, null)).join('') ?? '',
        )
      }
      // Determine column widths
      const columnWidths = tableToken.header.map((header, index) => {
        let maxWidth = getDisplayText(header.tokens).length
        for (const row of tableToken.rows) {
          const cellLen = getDisplayText(row[index]?.tokens).length
          maxWidth = Math.max(maxWidth, cellLen)
        }
        return Math.max(maxWidth, 3)
      })
      // Top border: ┌─────┬─────┐
      let out = '┌'
      columnWidths.forEach((width, i) => {
        out += '─'.repeat(width + 2)
        out += i < columnWidths.length - 1 ? '┬' : '┐'
      })
      out += EOL
      // Header row: │  A  │  B  │
      out += '│'
      tableToken.header.forEach((header, index) => {
        const content = header.tokens
          ?.map(t => formatToken(t, 0, null, null))
          .join('') ?? ''
        const displayLen = getDisplayText(header.tokens).length
        const width = columnWidths[index] ?? 3
        out += ' ' + padCell(content, displayLen, width) + ' │'
      })
      out += EOL
      // Header separator: ├─────┼─────┤
      out += '├'
      columnWidths.forEach((width, i) => {
        out += '─'.repeat(width + 2)
        out += i < columnWidths.length - 1 ? '┼' : '┤'
      })
      out += EOL
      // Data rows: │ 1   │ 2   │
      tableToken.rows.forEach((row, rowIdx) => {
        out += '│'
        row.forEach((cell, index) => {
          const content = cell.tokens
            ?.map(t => formatToken(t, 0, null, null))
            .join('') ?? ''
          const displayLen = getDisplayText(cell.tokens).length
          const width = columnWidths[index] ?? 3
          out += ' ' + padCell(content, displayLen, width) + ' │'
        })
        out += EOL
        // Row separator (between rows, not after last): ├─────┼─────┤
        if (rowIdx < tableToken.rows.length - 1) {
          out += '├'
          columnWidths.forEach((width, i) => {
            out += '─'.repeat(width + 2)
            out += i < columnWidths.length - 1 ? '┼' : '┤'
          })
          out += EOL
        }
      })
      // Bottom border: └─────┴─────┘
      out += '└'
      columnWidths.forEach((width, i) => {
        out += '─'.repeat(width + 2)
        out += i < columnWidths.length - 1 ? '┴' : '┘'
      })
      out += EOL
      return out + EOL
    }
    case 'escape':
      return token.text
    case 'image':
      return token.href
    default:
      return ''
  }
}

function padCell(content: string, displayWidth: number, targetWidth: number): string {
  const padding = Math.max(0, targetWidth - displayWidth)
  return content + ' '.repeat(padding)
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
