import { describe, test, expect } from 'bun:test'
import { renderMarkdown, formatToken } from '../src/render/markdown.js'
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
    expect(result).toContain('├')
    expect(result).toContain('┤')
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

  test('renders hr as horizontal line', () => {
    const result = stripAnsi(formatToken({ type: 'hr', raw: '---' } as Token))
    expect(result).toContain('---')
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

// ---------------------------------------------------------------------------
// splitMarkdownBlocks
// ---------------------------------------------------------------------------

import { splitMarkdownBlocks } from '../src/render/markdown.js'

describe('splitMarkdownBlocks', () => {
  test('empty text returns empty', () => {
    expect(splitMarkdownBlocks('')).toEqual({ completed: '', pending: '' })
  })

  test('single paragraph without blank line stays pending', () => {
    const result = splitMarkdownBlocks('hello world')
    expect(result.completed).toBe('')
    expect(result.pending).toBe('hello world')
  })

  test('two paragraphs split at blank line', () => {
    const text = 'paragraph one\n\nparagraph two'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('paragraph one\n\n')
    expect(result.pending).toBe('paragraph two')
  })

  test('multiple paragraphs split at last blank line', () => {
    const text = 'para one\n\npara two\n\npara three'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('para one\n\npara two\n\n')
    expect(result.pending).toBe('para three')
  })

  test('code fence keeps content pending until closed', () => {
    const text = 'intro\n\n```js\nconst x = 1\n```\n\nafter'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toContain('intro')
    expect(result.completed).toContain('```')
    expect(result.pending).toBe('after')
  })

  test('unclosed code fence keeps everything pending', () => {
    const text = 'intro\n\n```js\nconst x = 1\nmore code'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('')
    expect(result.pending).toBe(text)
  })

  test('trailing blank line makes everything completed', () => {
    const text = 'hello world\n\n'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('hello world\n\n')
    expect(result.pending).toBe('')
  })

  test('heading followed by paragraph', () => {
    const text = '# Title\n\nSome text\n\nMore text'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toContain('# Title')
    expect(result.completed).toContain('Some text')
    expect(result.pending).toBe('More text')
  })

  test('tilde code fence handled', () => {
    const text = 'before\n\n~~~\ncode\n~~~\n\nafter'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toContain('before')
    expect(result.completed).toContain('~~~')
    expect(result.pending).toBe('after')
  })
})
