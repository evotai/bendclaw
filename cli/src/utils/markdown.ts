/**
 * Markdown rendering for terminal output.
 * Uses marked lexer + chalk + cli-highlight for proper code blocks, tables, etc.
 * Approach modeled after Claude Code's formatToken.
 */

import chalk from 'chalk'
import { marked, type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import { linkifyIssueRefs } from './linkify.js'

const MARKDOWN_RENDER_CACHE_LIMIT = 256
const markdownRenderCache = new Map<string, string>()

interface CodeHighlighter {
  supportsLanguage(language: string): boolean
  highlight(code: string, options?: { language?: string }): string
}

const ANSI = {
  reset: '\u001b[0m',
  dim: '\u001b[2m',
  green: '\u001b[32m',
  yellow: '\u001b[33m',
  blue: '\u001b[34m',
  magenta: '\u001b[35m',
  cyan: '\u001b[36m',
}

function colorize(text: string, color: string): string {
  return `${color}${text}${ANSI.reset}`
}

function containsAnsi(text: string): boolean {
  return /\u001b\[[0-9;]*m/.test(text)
}

const fallbackLanguages = new Set([
  'js', 'jsx', 'ts', 'tsx', 'javascript', 'typescript',
  'json', 'bash', 'sh', 'shell', 'zsh',
  'rust', 'rs',
  'python', 'py',
  'go',
  'java',
  'sql',
  'yaml', 'yml',
  'toml',
  'markdown', 'md',
])

function fallbackHighlight(code: string, language?: string): string {
  let highlighted = code

  highlighted = highlighted.replace(/(#.*$|\/\/.*$)/gm, (match) => colorize(match, ANSI.dim))
  highlighted = highlighted.replace(/("[^"\n]*"|'[^'\n]*')/g, (match) => colorize(match, ANSI.green))
  highlighted = highlighted.replace(/\b(\d+(?:\.\d+)?)\b/g, (match) => colorize(match, ANSI.yellow))

  if (language) {
    const keywordPatterns: Record<string, RegExp> = {
      rust: /\b(fn|let|mut|pub|impl|trait|struct|enum|match|if|else|for|while|loop|return|use)\b/g,
      rs: /\b(fn|let|mut|pub|impl|trait|struct|enum|match|if|else|for|while|loop|return|use)\b/g,
      javascript: /\b(const|let|var|function|return|if|else|for|while|class|new|import|from|export|await|async)\b/g,
      js: /\b(const|let|var|function|return|if|else|for|while|class|new|import|from|export|await|async)\b/g,
      typescript: /\b(const|let|var|function|return|if|else|for|while|class|new|import|from|export|await|async|type|interface|implements)\b/g,
      ts: /\b(const|let|var|function|return|if|else|for|while|class|new|import|from|export|await|async|type|interface|implements)\b/g,
      python: /\b(def|class|return|if|elif|else|for|while|import|from|as|with|try|except|finally|async|await)\b/g,
      py: /\b(def|class|return|if|elif|else|for|while|import|from|as|with|try|except|finally|async|await)\b/g,
      bash: /\b(if|then|else|fi|for|do|done|case|esac|function|export|local)\b/g,
      sh: /\b(if|then|else|fi|for|do|done|case|esac|function|export|local)\b/g,
      shell: /\b(if|then|else|fi|for|do|done|case|esac|function|export|local)\b/g,
      zsh: /\b(if|then|else|fi|for|do|done|case|esac|function|export|local)\b/g,
      go: /\b(func|package|import|return|if|else|for|range|type|struct|interface|go|defer)\b/g,
      java: /\b(class|public|private|protected|static|final|void|return|if|else|for|while|new|import|package)\b/g,
      sql: /\b(SELECT|FROM|WHERE|GROUP BY|ORDER BY|INSERT|UPDATE|DELETE|CREATE|TABLE|JOIN|LEFT|RIGHT|INNER|OUTER|LIMIT)\b/g,
    }

    const pattern = keywordPatterns[language.trim().toLowerCase()]
    if (pattern) {
      highlighted = highlighted.replace(pattern, (match) => colorize(match, ANSI.cyan))
    }
  }

  return highlighted
}

const fallbackHighlighter: CodeHighlighter = {
  supportsLanguage(language: string): boolean {
    return fallbackLanguages.has(language.trim().toLowerCase())
  },
  highlight(code: string, options?: { language?: string }): string {
    return fallbackHighlight(code, options?.language)
  },
}

let highlighter: CodeHighlighter | null = fallbackHighlighter
try {
  highlighter = await import('cli-highlight')
} catch {
  // cli-highlight not available — fall back to a lightweight built-in highlighter
}

export function isCodeHighlightingAvailable(): boolean {
  return highlighter !== null
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
      const normalizedLang = lang?.trim().toLowerCase()
      if (normalizedLang === 'markdown' || normalizedLang === 'md') {
        return renderMarkdown(text) + EOL
      }
      let highlighted = text
      if (highlighter && lang) {
        try {
          if (highlighter.supportsLanguage(lang)) {
            highlighted = highlighter.highlight(text, { language: lang })
          }
        } catch {
          // fallback to plain text
        }
        if (!containsAnsi(highlighted) && fallbackHighlighter.supportsLanguage(lang)) {
          highlighted = fallbackHighlighter.highlight(text, { language: lang })
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

  const cached = markdownRenderCache.get(text)
  if (cached !== undefined) {
    return cached
  }

  configureMarked()
  try {
    const tokens = marked.lexer(text)
    const rendered = tokens
      .map(t => formatToken(t))
      .join('')
      .replace(/\n{3,}/g, '\n\n')
      .trimEnd()
    setMarkdownRenderCache(text, rendered)
    return rendered
  } catch {
    return text
  }
}

function setMarkdownRenderCache(input: string, rendered: string): void {
  markdownRenderCache.set(input, rendered)
  if (markdownRenderCache.size > MARKDOWN_RENDER_CACHE_LIMIT) {
    const oldestKey = markdownRenderCache.keys().next().value
    if (oldestKey !== undefined) {
      markdownRenderCache.delete(oldestKey)
    }
  }
}

export function clearMarkdownRenderCache(): void {
  markdownRenderCache.clear()
}

export function getMarkdownRenderCacheSize(): number {
  return markdownRenderCache.size
}
