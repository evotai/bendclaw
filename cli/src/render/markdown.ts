/**
 * Markdown rendering for terminal output.
 * Uses marked lexer + chalk + cli-highlight for proper code blocks, tables, etc.
 * Approach modeled after Claude Code's formatToken.
 */

import chalk from 'chalk'
import { marked, type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import wrapAnsi from 'wrap-ansi'
import { createHyperlink, isWarpTerminal, supportsHyperlinks, wrapHyperlink } from './hyperlink.js'
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
const SAFETY_MARGIN = 4
const MAX_TABLE_ROW_LINES = 4
const MAX_RENDER_WIDTH = 140
const CODE_FENCE_RE = /^( {0,3})(`{3,}|~{3,})(.*)$/
const MARKDOWN_BOUNDARY_RE = /^(#{1,6}\s|(?:[-*+]\s)|(?:\d+\.\s)|>\s|\|.*\||-{3,}\s*$)/
const CODE_LIKE_START_RE = /^[\[{(}\]),;]|^\/\/|^#\s*include\b/
const CODE_KEYWORD_RE = /^(return|if|else|for|while|switch|case|break|continue|try|catch|finally|throw|await|async|const|let|var|function|class|def|import|export|from|SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|WHERE|ORDER|GROUP|LIMIT)\b/i
const CODE_ASSIGNMENT_RE = /^[\w$.'"`-]+\s*[:=]/

function terminalContentWidth(): number {
  const columns = process.stdout.columns ?? 80
  return Math.max(20, Math.min(columns - SAFETY_MARGIN, MAX_RENDER_WIDTH))
}

function wrapDisplayLine(line: string, width: number): string[] {
  if (!line || width <= 0 || stringWidth(stripAnsi(line)) <= width) return [line]
  const wrapped = wrapAnsi(line, width, { hard: true, trim: false, wordWrap: true })
  return wrapped.split('\n')
}

function wrapDisplayText(text: string, width = terminalContentWidth()): string {
  return text
    .split(EOL)
    .flatMap(line => wrapDisplayLine(line, width))
    .join(EOL)
}

function wrapPlainTokenText(text: string): string {
  return wrapDisplayText(linkifyIssueRefs(text))
}

function looksLikeMarkdownBoundary(line: string): boolean {
  return MARKDOWN_BOUNDARY_RE.test(line.trimStart())
}

function isFenceLine(line: string, marker?: string, minLength?: number): boolean {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return false
  if (marker && match[2]![0] !== marker) return false
  if (minLength !== undefined && match[2]!.length < minLength) return false
  return true
}

function fenceLanguageFromLine(line: string): string | null {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return null
  const info = match[3]!.trim()
  return /^([A-Za-z0-9_+.#-]+)\s*$/.exec(info)?.[1] ?? null
}

function isLikelyFenceClose(line: string, marker: string, minLength: number): boolean {
  const match = CODE_FENCE_RE.exec(line)
  return !!match && match[2]![0] === marker && match[2]!.length >= minLength
}

function looksLikeStructuredCode(lines: string[], lang: string | null): boolean {
  const normalizedLang = lang?.toLowerCase()
  if (normalizedLang && /^(json|jsonc|javascript|js|typescript|ts|tsx|jsx|sql|python|py|rust|rs|go|java|c|cpp|c\+\+|csharp|cs|bash|sh|zsh|yaml|yml|toml|xml|html|css|diff)$/.test(normalizedLang)) {
    return true
  }

  const content = lines.join('\n').trim()
  if (!content) return false
  if (/^[\[{]/.test(content)) return true
  if (/^(SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|ALTER|DROP)\b/i.test(content)) return true
  if (/^(import|export|const|let|var|function|class|def|async|type|interface)\b/.test(content)) return true
  return false
}

function looksLikePlainMarkdownAfterCode(line: string): boolean {
  const trimmed = line.trim()
  if (!trimmed) return false
  if (looksLikeMarkdownBoundary(line)) return true
  if (CODE_LIKE_START_RE.test(trimmed)) return false
  if (CODE_KEYWORD_RE.test(trimmed)) return false
  if (CODE_ASSIGNMENT_RE.test(trimmed)) return false
  return /[\u4e00-\u9fff]/.test(trimmed) || /^[A-Z][\w\s,;:()/-]{12,}$/.test(trimmed)
}

function countStructuralBalance(lines: string[]): number {
  let balance = 0
  let inString: string | null = null
  let escaped = false
  for (const ch of lines.join('\n')) {
    if (inString) {
      if (escaped) {
        escaped = false
      } else if (ch === '\\') {
        escaped = true
      } else if (ch === inString) {
        inString = null
      }
      continue
    }
    if (ch === '"' || ch === "'") {
      inString = ch
    } else if (ch === '{' || ch === '[' || ch === '(') {
      balance++
    } else if (ch === '}' || ch === ']' || ch === ')') {
      balance--
    }
  }
  return balance
}

function looksLikeCodeCompleted(lines: string[], lang: string | null): boolean {
  const nonBlank = lines.filter(line => line.trim().length > 0)
  if (nonBlank.length === 0) return false
  const last = nonBlank[nonBlank.length - 1]!.trim()
  if (/^[}\]\);,]*$/.test(last)) return countStructuralBalance(nonBlank) <= 0
  if (lang?.toLowerCase() === 'sql' && /;$/.test(last)) return true
  return false
}

function shouldCloseOpenFenceBeforeLine(line: string, codeLines: string[], lang: string | null): boolean {
  if (!looksLikeStructuredCode(codeLines, lang)) return false
  if (looksLikeMarkdownBoundary(line)) return looksLikeCodeCompleted(codeLines, lang)
  if (!looksLikeCodeCompleted(codeLines, lang)) return false
  return looksLikePlainMarkdownAfterCode(line)
}

function repairUnclosedFences(content: string, finalClose: boolean): string {
  const lines = content.split('\n')
  let out = ''
  let openMarker = ''
  let openLength = 0
  let openClose = ''
  let openLang: string | null = null
  let codeLines: string[] = []

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!
    const newline = i < lines.length - 1 ? '\n' : ''
    const match = CODE_FENCE_RE.exec(line)

    if (!openMarker) {
      if (match) {
        openMarker = match[2]![0]!
        openLength = match[2]!.length
        openClose = openMarker.repeat(openLength)
        openLang = fenceLanguageFromLine(line)
        codeLines = []
      }
      out += line + newline
      continue
    }

    if (isLikelyFenceClose(line, openMarker, openLength)) {
      openMarker = ''
      openLength = 0
      openClose = ''
      openLang = null
      codeLines = []
      out += line + newline
      continue
    }

    if (!isFenceLine(line) && shouldCloseOpenFenceBeforeLine(line, codeLines, openLang)) {
      out += `${openClose}\n`
      openMarker = ''
      openLength = 0
      openClose = ''
      openLang = null
      codeLines = []
    }

    out += line + newline
    if (openMarker) {
      codeLines.push(line)
    }
  }

  if (finalClose && openMarker) {
    out += out.endsWith('\n') ? openClose : `\n${openClose}`
  }
  return out
}

function prepareMarkdownForLex(text: string): string {
  return repairUnclosedFences(text, true)
}

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
      return wrapDisplayText(highlighted) + EOL
    }
    case 'codespan': {
      const raw = token.text as string
      const isFilePath = /^[~/][\w./_-]+$/.test(raw)
      // Warp auto-detects file paths in plain text; ANSI codes break detection.
      // Skip coloring for file paths unless hyperlinks are force-enabled.
      if (isFilePath && isWarpTerminal() && process.env.FORCE_HYPERLINK !== '1') {
        return raw
      }
      const colored = chalk.hex('#5fb3b3')(raw)
      // Make absolute file paths clickable (file:// hyperlink)
      if (supportsHyperlinks() && isFilePath) {
        const resolved = raw.startsWith('~')
          ? raw.replace('~', process.env.HOME ?? '~')
          : raw
        return wrapHyperlink(`file://${resolved}`, colored)
      }
      return colored
    }
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
        return chalk.hex('#c0c0c0').bold.italic.underline(text) + EOL
      }
      return chalk.hex('#c0c0c0').bold(text) + EOL
    }
    case 'hr':
      return `---${EOL}`
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
        wrapDisplayText((token.tokens ?? [])
          .map(t => formatToken(t, 0, null, null))
          .join('')) + EOL
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
      return wrapPlainTokenText(token.text)
    }
    case 'table': {
      const tableToken = token as Tokens.Table
      const numCols = tableToken.header.length
      const termWidth = terminalContentWidth()
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
          lines.push(...wrapDisplayLine(srcLine, width))
        }
        return lines.length > 0 ? lines : ['']
      }

      // --- check if vertical format is needed ---
      const MAX_ROW_LINES = MAX_TABLE_ROW_LINES
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
            out += `${header}: ${wrapDisplayText(value)}${EOL}`
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

const BLOCK_TYPES = new Set([
  'paragraph', 'code', 'heading', 'list', 'blockquote', 'hr', 'table',
])

function formatTokens(tokens: Token[]): string {
  let out = ''
  let prevWasBlock = false

  for (const token of tokens) {
    const rendered = formatToken(token)
    if (!rendered) continue
    const isBlock = BLOCK_TYPES.has(token.type)
    // Insert blank line between consecutive block-level elements
    if (isBlock && prevWasBlock) {
      out += EOL
    }
    out += rendered
    prevWasBlock = isBlock
  }

  return out.trim()
}

/**
 * Render markdown text to terminal-friendly ANSI output.
 */
export function renderMarkdown(text: string): string {
  if (!text || text.trim().length === 0) return text

  configureMarked()
  try {
    const lexText = prepareMarkdownForLex(text)
    const tokens = hasMarkdownSyntax(lexText)
      ? marked.lexer(lexText)
      : plainTextTokens(text)
    return insertWordBoundaries(formatTokens(tokens))
  } catch {
    return text
  }
}

// ---------------------------------------------------------------------------
// Word boundary insertion for CJK text
// ---------------------------------------------------------------------------

// CJK Unified Ideographs, CJK Extension A, CJK Compat Ideographs
const CJK_IDEO = '[\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff]'
const ASCII_RE = '[\x21-\x7e]'
// CJK punctuation: CJK Symbols, Fullwidth punctuation, quotation marks, etc.
const CJK_PUNCT = '[\u3001-\u3003\u3008-\u3011\u3014-\u301f\uff01-\uff0f\uff1a-\uff20\uff3b-\uff40\uff5b-\uff65\u2018-\u201f\u2026\u2014\u3000\uff0c\uff0e]'
const ZWSP = '\u200B'
const CJK_TO_ASCII = new RegExp(`(${CJK_IDEO})(${ASCII_RE})`, 'g')
const ASCII_TO_CJK = new RegExp(`(${ASCII_RE})(${CJK_IDEO})`, 'g')
const CJK_PUNCT_RE = new RegExp(`(${CJK_PUNCT})`, 'g')
const DOUBLE_ZWSP = /\u200B\u200B+/g

/**
 * Insert zero-width spaces at CJK / ASCII boundaries and around CJK
 * punctuation so terminal double-click word selection stops correctly.
 */
export function insertWordBoundaries(s: string): string {
  return s
    .replace(ASCII_TO_CJK, `$1${ZWSP}$2`)
    .replace(CJK_TO_ASCII, `$1${ZWSP}$2`)
    .replace(CJK_PUNCT_RE, `${ZWSP}$1${ZWSP}`)
    .replace(DOUBLE_ZWSP, ZWSP)
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

  const commitPoint = findStreamingCommitPoint(text)
  return {
    completed: text.slice(0, commitPoint),
    pending: text.slice(commitPoint),
  }
}

export function findStreamingCommitPoint(text: string): number {
  if (!text) return 0

  const repaired = repairUnclosedFences(text, false)
  if (repaired !== text) {
    const insertedAt = firstDifferenceIndex(text, repaired)
    return insertedAt > 0 ? insertedAt : 0
  }

  configureMarked()
  const tokens = marked.lexer(text)
  let lastContentIdx = tokens.length - 1
  while (lastContentIdx >= 0 && tokens[lastContentIdx]!.type === 'space') {
    lastContentIdx--
  }
  if (lastContentIdx <= 0) return text.endsWith('\n\n') ? text.length : 0

  let splitAt = 0
  for (let i = 0; i < lastContentIdx; i++) {
    splitAt += tokens[i]!.raw.length
  }
  if (splitAt <= 0 || splitAt >= text.length) return text.endsWith('\n\n') ? text.length : 0
  return splitAt
}

function firstDifferenceIndex(a: string, b: string): number {
  const limit = Math.min(a.length, b.length)
  for (let i = 0; i < limit; i++) {
    if (a[i] !== b[i]) return i
  }
  return limit
}
