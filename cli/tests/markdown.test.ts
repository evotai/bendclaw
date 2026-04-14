import { describe, test, expect } from 'bun:test'
import {
  renderMarkdown,
  formatToken,
  clearMarkdownRenderCache,
  getMarkdownRenderCacheSize,
} from '../src/utils/markdown.js'
import { marked, type Token } from 'marked'
import stripAnsi from 'strip-ansi'

// Helper: render markdown and strip ANSI codes for assertion
function render(md: string): string {
  return stripAnsi(renderMarkdown(md))
}

// Helper: lex a single token from markdown
function lexFirst(md: string): Token {
  const tokens = marked.lexer(md)
  return tokens[0]!
}

describe('renderMarkdown', () => {
  test('memoizes rendered markdown output for repeated inputs', () => {
    clearMarkdownRenderCache()
    expect(getMarkdownRenderCacheSize()).toBe(0)

    const input = '## Title\n\n- one\n- two'
    const first = renderMarkdown(input)
    expect(getMarkdownRenderCacheSize()).toBe(1)

    const second = renderMarkdown(input)
    expect(second).toBe(first)
    expect(getMarkdownRenderCacheSize()).toBe(1)
  })

  test('renders plain text', () => {
    expect(render('hello world')).toBe('hello world')
  })

  test('returns empty/whitespace input as-is', () => {
    expect(renderMarkdown('')).toBe('')
    expect(renderMarkdown('  ')).toBe('  ')
  })

  test('renders headings', () => {
    const result = render('# Title')
    expect(result).toContain('Title')
  })

  test('renders h2', () => {
    const result = render('## Subtitle')
    expect(result).toContain('Subtitle')
  })

  test('renders bold text', () => {
    const result = render('this is **bold** text')
    expect(result).toContain('bold')
  })

  test('renders italic text', () => {
    const result = render('this is *italic* text')
    expect(result).toContain('italic')
  })

  test('renders inline code', () => {
    const result = render('use `foo()` here')
    expect(result).toContain('foo()')
  })

  test('renders code blocks', () => {
    const result = render('```js\nconst x = 1\n```')
    expect(result).toContain('const x = 1')
  })

  test('loads the runtime code highlighter for fenced code blocks', async () => {
    const markdown = await import('../src/utils/markdown.js')
    expect(typeof markdown.isCodeHighlightingAvailable).toBe('function')
    expect(markdown.isCodeHighlightingAvailable()).toBe(true)
  })

  test('emits ANSI styling for supported language code fences', () => {
    const result = renderMarkdown('```rust\nfn main() { let x = 1 }\n```')
    expect(result).toContain('\u001b[')
    expect(stripAnsi(result)).toContain('fn main() { let x = 1 }')
  })

  test('renders markdown fenced blocks as nested markdown', () => {
    const result = render('```markdown\n| A | B |\n|---|---|\n| 1 | 2 |\n```')
    expect(result).toContain('┌')
    expect(result).toContain('│ A')
    expect(result).toContain('│ 1')
    expect(result).not.toContain('```')
  })

  test('renders md fenced blocks as nested markdown', () => {
    const result = render('```md\n- one\n- two\n```')
    expect(result).toContain('- one')
    expect(result).toContain('- two')
    expect(result).not.toContain('```')
  })

  test('renders unordered lists', () => {
    const result = render('- one\n- two\n- three')
    expect(result).toContain('- one')
    expect(result).toContain('- two')
    expect(result).toContain('- three')
  })

  test('renders ordered lists', () => {
    const result = render('1. first\n2. second')
    expect(result).toContain('1.')
    expect(result).toContain('first')
    expect(result).toContain('second')
  })

  test('renders blockquotes', () => {
    const result = render('> quoted text')
    expect(result).toContain('quoted text')
  })

  test('renders links', () => {
    const result = render('[click](https://example.com)')
    expect(result).toContain('click')
    expect(result).toContain('https://example.com')
  })

  test('renders horizontal rules', () => {
    const result = render('---')
    expect(result).toContain('---')
  })

  test('renders tables with box-drawing characters', () => {
    const md = '| A | B |\n|---|---|\n| 1 | 2 |'
    const result = render(md)
    expect(result).toContain('A')
    expect(result).toContain('B')
    expect(result).toContain('1')
    expect(result).toContain('2')
    expect(result).toContain('┌')
    expect(result).toContain('┐')
    expect(result).toContain('│')
    expect(result).toContain('└')
    expect(result).toContain('┘')
  })

  test('collapses excessive newlines', () => {
    const result = renderMarkdown('hello\n\n\n\nworld')
    expect(result).not.toContain('\n\n\n')
  })

  test('falls back to raw text on parse error', () => {
    // renderMarkdown should never throw
    const result = renderMarkdown('just plain text')
    expect(result).toContain('just plain text')
  })
})

describe('formatToken', () => {
  test('renders paragraph token', () => {
    const token = lexFirst('hello world')
    const result = stripAnsi(formatToken(token))
    expect(result).toContain('hello world')
  })

  test('renders space token as newline', () => {
    const result = formatToken({ type: 'space', raw: '\n\n' } as Token)
    expect(result).toBe('\n')
  })

  test('renders br token as newline', () => {
    const result = formatToken({ type: 'br', raw: '\n' } as Token)
    expect(result).toBe('\n')
  })

  test('renders escape token as text', () => {
    const result = formatToken({ type: 'escape', raw: '\\)', text: ')' } as Token)
    expect(result).toBe(')')
  })

  test('renders hr as ---', () => {
    const result = formatToken({ type: 'hr', raw: '---' } as Token)
    expect(result).toBe('---')
  })

  test('renders image as href', () => {
    const result = formatToken({ type: 'image', raw: '![alt](url)', href: 'https://img.png', text: 'alt' } as Token)
    expect(result).toBe('https://img.png')
  })

  test('returns empty string for unknown token types', () => {
    const result = formatToken({ type: 'html', raw: '<div>' } as Token)
    expect(result).toBe('')
  })
})
