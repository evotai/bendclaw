import { marked, type Token } from 'marked'
import { renderMarkdown } from './markdown.js'

const UNCLOSED_FENCE = '\n```'

export function renderStreamingText(text: string): string {
  return renderMarkdown(normalizeStreamingMarkdown(text))
}

export function splitStreamingMarkdown(text: string, stablePrefix: string): {
  stablePrefix: string
  unstableSuffix: string
} {
  if (!text.startsWith(stablePrefix)) {
    stablePrefix = ''
  }

  const boundary = stablePrefix.length
  const tokens = marked.lexer(text.slice(boundary))
  let lastContentIdx = tokens.length - 1
  while (lastContentIdx >= 0 && tokens[lastContentIdx]?.type === 'space') {
    lastContentIdx--
  }

  let advance = 0
  for (let i = 0; i < lastContentIdx; i++) {
    advance += tokenRawLength(tokens[i]!)
  }

  const nextStablePrefix = text.slice(0, boundary + advance)
  return {
    stablePrefix: nextStablePrefix,
    unstableSuffix: text.slice(nextStablePrefix.length),
  }
}

export function shouldRefreshStreamingMarkdown(lastRenderedAt: number, now: number, minIntervalMs: number): boolean {
  return lastRenderedAt === 0 || (now - lastRenderedAt) >= minIntervalMs
}

function normalizeStreamingMarkdown(text: string): string {
  const fenceCount = (text.match(/```/g) ?? []).length
  if (fenceCount % 2 === 1) {
    return text + UNCLOSED_FENCE
  }
  return text
}

function tokenRawLength(token: Token): number {
  return typeof token.raw === 'string' ? token.raw.length : 0
}

export function shouldAnimateTerminalTitle(): boolean {
  return process.env.EVOT_ANIMATE_TITLE === '1'
}
