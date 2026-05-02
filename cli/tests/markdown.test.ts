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

  test('renders unclosed code fence as code', () => {
    const result = render('```sql\nSELECT 1')
    expect(result).toContain('SELECT 1')
    expect(result).not.toContain('```')
  })

  test('renders unclosed tilde fence as code', () => {
    const result = render('~~~sql\nSELECT 1')
    expect(result).toContain('SELECT 1')
    expect(result).not.toContain('~~~')
  })

  test('repairs unclosed code fence before later prose', () => {
    const md = '```json\n[\n  {"id":"evt-001"}\n]\n\n原样保存，没有任何转换。'
    const result = render(md)
    expect(result).toContain('{"id":"evt-001"}')
    expect(result).toContain('原样保存')
    expect(result).not.toContain('```')
  })

  test('repairs unclosed fence before following markdown heading without a blank line', () => {
    const result = render('```json\n{\n  "id": "tr-abc"\n}\n## 第 8 站：补充 input / output')

    expect(result).toContain('"id": "tr-abc"')
    expect(result.replace(/\u200b/g, '')).toContain('第 8 站：补充 input / output')
    expect(result).not.toContain('```json')
  })

  test('repairs completed json fence before plain chinese paragraph', () => {
    const result = render('最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n原始事件继续说明')

    expect(result).toContain('"is_deleted": 0')
    expect(result).toContain('原始事件继续说明')
    expect(result).not.toContain('```json')
  })

  test('repairs completed json fence before markdown hr without a blank line', () => {
    const result = render('最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n---\n第 8 站：补充 input / output')
      .replace(/\u200b/g, '')

    expect(result).toContain('"is_deleted": 0')
    expect(result).toContain('---\n\n第 8 站：补充 input / output')
    expect(result).not.toContain('```json')
  })

  test('keeps adjacent prose compact', () => {
    const result = render('第一行\n第二行\n第三行')

    expect(result).toBe('第一行\n第二行\n第三行')
  })

  test('keeps list items compact', () => {
    const result = render('- 第一项\n- 第二项\n- 第三项')

    expect(result).toBe('- 第一项\n- 第二项\n- 第三项')
  })

  test('wraps very long plain lines', () => {
    const prev = process.stdout.columns
    process.stdout.columns = 40
    try {
      const result = render('INSERT ' + 'x'.repeat(80))
      expect(result.split('\n').length).toBeGreaterThan(1)
    } finally {
      process.stdout.columns = prev
    }
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
    // URL may be inside an OSC 8 hyperlink (stripped by stripAnsi) or shown as fallback text
    const raw = renderMarkdown('[click](https://example.com)')
    expect(raw).toContain('https://example.com')
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
// File path linkification in codespan and text
// ---------------------------------------------------------------------------

describe('file path linkification', () => {
  const OSC8_START = '\x1b]8;;'

  test('codespan with absolute path produces file:// hyperlink', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '1'
    try {
      const result = renderMarkdown('see `/tmp/simple.md`')
      expect(result).toContain(OSC8_START)
      expect(result).toContain('file:///tmp/simple.md')
      // The path text should still be present
      expect(stripAnsi(result)).toContain('/tmp/simple.md')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('codespan with non-path content does not linkify', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '1'
    try {
      const result = renderMarkdown('use `foo()` here')
      expect(result).not.toContain(OSC8_START)
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('plain text with absolute path produces file:// hyperlink', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '1'
    try {
      const result = renderMarkdown('已生成：/tmp/simple.md')
      expect(result).toContain(OSC8_START)
      expect(result).toContain('file:///tmp/simple.md')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('no hyperlink when FORCE_HYPERLINK=0', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '0'
    try {
      const result = renderMarkdown('see `/tmp/simple.md`')
      expect(result).not.toContain(OSC8_START)
      expect(stripAnsi(result)).toContain('/tmp/simple.md')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })
})

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
    expect(result.completed).toBe('intro\n\n')
    expect(result.pending).toBe('```js\nconst x = 1\nmore code')
  })

  test('unclosed code fence can commit following markdown after heuristic repair', () => {
    const text = '```json\n[\n  {"id":"evt-001"}\n]\n\n## next'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('```json\n[\n  {"id":"evt-001"}\n]\n\n')
    expect(result.pending).toBe('## next')
  })

  test('unclosed code fence can commit following horizontal rule after heuristic repair', () => {
    const text = '最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n---\n第 8 站：补充 input / output'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n')
    expect(result.pending).toBe('---\n第 8 站：补充 input / output')
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
