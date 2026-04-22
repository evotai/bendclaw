/**
 * Markdown rendering for terminal output.
 * Uses marked lexer + chalk + cli-highlight for proper code blocks, tables, etc.
 * Approach modeled after Claude Code's formatToken.
 */

import chalk from 'chalk'
import { marked, type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import { createHyperlink, supportsHyperlinks } from './hyperlink.js'
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

  // Disable strikethrough parsing — the model often uses ~ for "approximate"
  // (e.g., ~100) and rarely intends actual strikethrough formatting.
  marked.use({
    tokenizer: {
      del() {
        return undefined as unknown as Tokens.Del
      },
    },
  })
}

// ---------------------------------------------------------------------------
// Markdown syntax fast-path detection
// ---------------------------------------------------------------------------

// Characters/patterns that indicate markdown syntax. If none are present,
// skip the marked.lexer call entirely — render as a single paragraph.
// Covers the majority of short assistant responses that are plain sentences.
// Ordered-list pattern requires `N. ` (digit + dot + space) to avoid
// misinterpreting bare "2." as a list item.
const MD_SYNTAX_RE = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. /

function hasMarkdownSyntax(s: string): boolean {
  return MD_SYNTAX_RE.test(s.length > 500 ? s.slice(0, 500) : s)
}

/** Build a plain-text paragraph token (no marked.lexer overhead). */
function plainTextTokens(content: string): Token[] {
  return [{
    type: 'paragraph',
    raw: content,
    text: content,
    tokens: [{ type: 'text', raw: content, text: content }],
  } as Token]
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
      return highlighted + EOL
    }
    case 'codespan':
      return chalk.cyan(token.text)
    case 'del':
      // del is disabled via configureMarked; if somehow reached, render as-is
      return ''
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
      if ((token as Tokens.Heading).depth === 1) {
        return chalk.bold.italic.underline(text) + EOL + EOL
      }
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
      // If the terminal supports OSC 8 hyperlinks, render as clickable link
      if (supportsHyperlinks()) {
        if (plainText && plainText !== token.href) {
          return createHyperlink(token.href, plainText)
        }
        return createHyperlink(token.href)
      }
      // Fallback: show text + dimmed URL, or underlined URL
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
      const tableToken = token as Tokens.Table
      const numCols = tableToken.header.length
      const termWidth = process.stdout.columns ?? 80
      const MIN_COL = 3

      // --- helpers ---
      function renderCell(tokens: Token[] | undefined): string {
        return tokens?.map(t => formatToken(t, 0, null, null)).join('').trimEnd() ?? ''
      }
      function plainText(tokens: Token[] | undefined): string {
        return stripAnsi(renderCell(tokens))
      }
      function longestWord(tokens: Token[] | undefined): number {
        const words = plainText(tokens).split(/\s+/).filter(w => w.length > 0)
        if (words.length === 0) return MIN_COL
        return Math.max(...words.map(w => stringWidth(w)), MIN_COL)
      }
      function idealWidth(tokens: Token[] | undefined): number {
        return Math.max(stringWidth(plainText(tokens)), MIN_COL)
      }

      // --- column width calculation ---
      const minWidths = tableToken.header.map((h, ci) => {
        let w = longestWord(h.tokens)
        for (const row of tableToken.rows) w = Math.max(w, longestWord(row[ci]?.tokens))
        return w
      })
      const idealWidths = tableToken.header.map((h, ci) => {
        let w = idealWidth(h.tokens)
        for (const row of tableToken.rows) w = Math.max(w, idealWidth(row[ci]?.tokens))
        return w
      })

      // border overhead: │ cell │ cell │ = 1 + numCols * 3
      const borderOverhead = 1 + numCols * 3
      const available = Math.max(termWidth - borderOverhead - 2, numCols * MIN_COL)
      const totalIdeal = idealWidths.reduce((s, w) => s + w, 0)
      const totalMin = minWidths.reduce((s, w) => s + w, 0)

      let colWidths: number[]
      if (totalIdeal <= available) {
        colWidths = idealWidths
      } else if (totalMin > available) {
        const each = Math.floor(available / numCols)
        colWidths = minWidths.map(() => Math.max(each, MIN_COL))
      } else {
        // give each column its min, distribute remaining proportionally
        colWidths = [...minWidths]
        let remaining = available - totalMin
        const extras = idealWidths.map((ideal, i) => ideal - minWidths[i]!)
        const totalExtra = extras.reduce((s, e) => s + e, 0)
        if (totalExtra > 0) {
          for (let i = 0; i < numCols; i++) {
            const share = Math.floor((extras[i]! / totalExtra) * remaining)
            colWidths[i] = colWidths[i]! + share
          }
        }
      }

      // --- ANSI-aware word wrap (CJK-safe) ---
      function wrapCell(text: string, width: number): string[] {
        if (width <= 0) return [text]
        const lines: string[] = []
        for (const srcLine of text.split('\n')) {
          const plain = stripAnsi(srcLine)
          if (stringWidth(plain) <= width) {
            lines.push(srcLine)
            continue
          }
          const segments = srcLine.split(/(\s+)/)
          let cur = ''
          let curW = 0
          for (const seg of segments) {
            const segW = stringWidth(stripAnsi(seg))
            // Segment fits on current line
            if (curW + segW <= width) {
              cur += seg
              curW += segW
              continue
            }
            // Segment doesn't fit — flush current line if non-empty
            if (curW > 0) {
              lines.push(cur)
              cur = ''
              curW = 0
            }
            // If segment itself fits in one line, start new line with it
            if (segW <= width) {
              cur = seg.trimStart()
              curW = stringWidth(stripAnsi(cur))
              continue
            }
            // Segment exceeds width — break character by character
            for (const ch of stripAnsi(seg)) {
              const chW = stringWidth(ch)
              if (curW + chW > width && curW > 0) {
                lines.push(cur)
                cur = ch
                curW = chW
              } else {
                cur += ch
                curW += chW
              }
            }
          }
          if (cur) lines.push(cur)
        }
        return lines.length > 0 ? lines : ['']
      }

      // --- check if vertical format is needed ---
      const MAX_ROW_LINES = 4
      let needVertical = false
      for (const row of tableToken.rows) {
        for (let ci = 0; ci < numCols; ci++) {
          const wrapped = wrapCell(renderCell(row[ci]?.tokens), colWidths[ci]!)
          if (wrapped.length > MAX_ROW_LINES) { needVertical = true; break }
        }
        if (needVertical) break
      }

      if (needVertical) {
        // vertical key-value format
        let out = ''
        tableToken.rows.forEach((row, ri) => {
          if (ri > 0) out += EOL
          for (let ci = 0; ci < numCols; ci++) {
            const header = chalk.bold(plainText(tableToken.header[ci]?.tokens))
            const value = renderCell(row[ci]?.tokens)
            out += `${header}: ${value}${EOL}`
          }
        })
        return out + EOL
      }

      // --- horizontal table with wrapping ---
      function borderLine(left: string, mid: string, cross: string, right: string): string {
        let line = left
        colWidths.forEach((w, i) => {
          line += mid.repeat(w + 2)
          line += i < numCols - 1 ? cross : right
        })
        return line
      }
      function renderRow(cells: { tokens?: Token[] }[]): string {
        const wrapped = cells.map((cell, ci) =>
          wrapCell(renderCell(cell.tokens), colWidths[ci]!),
        )
        const height = Math.max(...wrapped.map(w => w.length))
        const lines: string[] = []
        for (let li = 0; li < height; li++) {
          let line = '│'
          for (let ci = 0; ci < numCols; ci++) {
            const content = wrapped[ci]![li] ?? ''
            const dw = stringWidth(stripAnsi(content))
            const align = tableToken.align?.[ci]
            line += ' ' + padAligned(content, dw, colWidths[ci]!, align) + ' │'
          }
          lines.push(line)
        }
        return lines.join(EOL)
      }

      let out = borderLine('┌', '─', '┬', '┐') + EOL
      out += renderRow(tableToken.header) + EOL
      out += borderLine('├', '─', '┼', '┤') + EOL
      tableToken.rows.forEach((row, ri) => {
        out += renderRow(row) + EOL
        if (ri < tableToken.rows.length - 1) {
          out += borderLine('├', '─', '┼', '┤') + EOL
        }
      })
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
    const tokens = hasMarkdownSyntax(text)
      ? marked.lexer(text)
      : plainTextTokens(text)
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
// Markdown render cache (LRU)
// ---------------------------------------------------------------------------

const CACHE_MAX = 200
const renderCache = new Map<string, string>()

function simpleHash(s: string): string {
  let h = 0
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0
  }
  return h.toString(36)
}

/**
 * Render markdown with LRU caching.
 * Same as renderMarkdown but caches results by content hash.
 */
export function renderMarkdownCached(text: string): string {
  if (!text || text.trim().length === 0) return text

  const hash = simpleHash(text)
  const cached = renderCache.get(hash)
  if (cached !== undefined) {
    // Move to end (LRU touch)
    renderCache.delete(hash)
    renderCache.set(hash, cached)
    return cached
  }

  const result = renderMarkdown(text)

  renderCache.set(hash, result)
  if (renderCache.size > CACHE_MAX) {
    // Evict oldest entry
    const first = renderCache.keys().next().value
    if (first !== undefined) renderCache.delete(first)
  }

  return result
}

/** Clear the render cache (for tests). */
export function clearRenderCache(): void {
  renderCache.clear()
}

/** Get current cache size (for tests). */
export function getRenderCacheSize(): number {
  return renderCache.size
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
