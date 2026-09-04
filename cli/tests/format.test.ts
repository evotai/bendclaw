import { describe, test, expect } from 'bun:test'
import stringWidth from 'string-width'
import { padRight, relativeTime, renderBar, renderPositionBar } from '../src/render/format.js'

describe('padRight', () => {
  test('pads short string with spaces', () => {
    expect(padRight('hi', 6)).toBe('hi    ')
  })

  test('returns string as-is when exact length', () => {
    expect(padRight('hello', 5)).toBe('hello')
  })

  test('truncates with ellipsis when too long', () => {
    expect(padRight('hello world', 8)).toBe('hello w…')
  })

  test('handles empty string', () => {
    expect(padRight('', 4)).toBe('    ')
  })

  test('handles n=0', () => {
    expect(padRight('hi', 0)).toBe('…')
  })

  test('handles n=1 with long string', () => {
    expect(padRight('hello', 1)).toBe('…')
  })

  test('handles non-finite width', () => {
    expect(padRight('hi', Infinity)).toBe('…')
    expect(padRight('hi', NaN)).toBe('…')
  })

  test('measures wide characters by display width, not code point count', () => {
    // Two columns per CJK glyph: four glyphs fill an 8-column field exactly.
    expect(padRight('分析问题', 8)).toBe('分析问题')
    expect(padRight('分析', 8)).toBe('分析    ')
    // Truncation keeps whole glyphs and leaves room for the ellipsis, so the
    // result never exceeds the column budget.
    expect(stringWidth(padRight('分析为啥这么多untitled的session', 12))).toBeLessThanOrEqual(12)
  })

  test('padded output measures exactly the requested width', () => {
    // The fast width path must agree with string-width for every alphabet, or
    // columns drift and table rows stop lining up.
    for (const sample of ['hi', '中文', 'a👍b', 'e\u0301', '분석', 'ｱｲｳ', 'àéî', 'ΑΒΓ', 'АБВ']) {
      expect(stringWidth(padRight(sample, 20))).toBe(20)
    }
  })

  test('pads and truncates whole Unicode graphemes at the requested width', () => {
    for (const sample of ['नमस्ते', '가', 'が', 'क्‍ष']) {
      expect(stringWidth(padRight(sample, 8))).toBe(8)
    }
    expect(padRight('नमस्ते', 2)).toBe('न…')
    expect(padRight('가x', 2)).toBe('…')
  })

  test('matches a grapheme-aware string-width reference across mixed scripts', () => {
    const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' })
    const reference = (s: string, n: number): string => {
      n = Number.isFinite(n) ? Math.max(0, Math.floor(n)) : 0
      const w = stringWidth(s)
      if (w > n) {
        let truncated = ''
        let tw = 0
        for (const { segment } of segmenter.segment(s)) {
          const cw = stringWidth(segment)
          if (tw + cw > n - 1) break
          truncated += segment
          tw += cw
        }
        return truncated + '…'
      }
      return s + ' '.repeat(Math.max(0, n - w))
    }

    const alphabets = [
      'abcdefghijklmnopqrstuvwxyz0123456789 .,-_/:()[]{}',
      '分析为啥这么多untitle的session这些不应该生成才对',
      'こんにちは世界カタカナ', '한국어테스트', 'àéîõüñçßøåæœ', 'ΑΒΓΔΕΖ', 'АБВГДЕ',
      '👨‍👩‍👧‍👦🎉🚀🇯🇵', 'e\u0301a\u0300o\u0308', '（）［］：；！？．，', '①②③⑩Ⅷ', '☃★♠♥⚡✓✗…—–',
      'ｱｲｳｴｵﬀﬁ', 'नमस्ते', '가', 'が', 'क्‍ष',
    ]
    for (let i = 0; i < 600; i++) {
      let sample = ''
      for (let j = 0; j < i % 40; j++) {
        const alpha = alphabets[(i * 7 + j * 13) % alphabets.length]!
        sample += alpha[(i * 31 + j * 17) % alpha.length]
      }
      for (const width of [0, 1, 3, 6, 12, 44, 80]) {
        expect(padRight(sample, width)).toBe(reference(sample, width))
      }
    }
  })
})

describe('renderBar', () => {
  test('handles non-finite inputs without throwing', () => {
    expect(renderBar(5, 10, Infinity)).toBe('')
    expect(renderBar(Infinity, 10, 4)).toBe('░░░░')
    expect(renderBar(5, NaN, 4)).toBe('░░░░')
  })
})

describe('renderPositionBar', () => {
  test('keeps unchanged marker consistent for L3', () => {
    const { bar, legend } = renderPositionBar(10, [{ index: 2, end_index: 4, method: 'MessagesEvicted' }], 3)
    expect(bar).toBe('[··EEE·····]')
    expect(legend).toBe('·=unchanged/kept  E=MessagesEvicted')
  })

  test('kept ranges visible when proportional mapping would hide them', () => {
    // 251 messages, indices 2–240 dropped, kept: [0,1] and [241,250]
    const actions = [{ index: 2, end_index: 240, method: 'MessagesEvicted' }]
    const { bar } = renderPositionBar(251, actions, 3)
    // Both kept ranges must have at least one '·'
    const chars = bar.slice(1, -1) // strip [ ]
    expect(chars.length).toBe(40)
    // First kept range [0,1] → slot 0 must be '·'
    expect(chars[0]).toBe('·')
    // Last kept range [241,250] → last slot(s) must include '·'
    const lastDot = chars.lastIndexOf('·')
    expect(lastDot).toBeGreaterThan(chars.length - 3) // near the end
  })

  test('no kept ranges means all action slots', () => {
    // Every message has an action — no gaps to preserve
    const actions = [{ index: 0, end_index: 99, method: 'MessagesEvicted' }]
    const { bar } = renderPositionBar(100, actions, 3)
    const chars = bar.slice(1, -1)
    expect(chars.length).toBe(40)
    expect(chars).not.toContain('·')
    expect(chars).toContain('─100─')
  })
})

describe('relativeTime', () => {
  test('returns "just now" for recent timestamps', () => {
    const now = new Date().toISOString()
    expect(relativeTime(now)).toBe('just now')
  })

  test('returns minutes ago', () => {
    const fiveMinAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString()
    expect(relativeTime(fiveMinAgo)).toBe('5m ago')
  })

  test('returns hours ago', () => {
    const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString()
    expect(relativeTime(twoHoursAgo)).toBe('2h ago')
  })

  test('returns days ago', () => {
    const threeDaysAgo = new Date(Date.now() - 3 * 24 * 60 * 60 * 1000).toISOString()
    expect(relativeTime(threeDaysAgo)).toBe('3d ago')
  })

  test('returns raw string on invalid input', () => {
    expect(relativeTime('not-a-date')).toBe('not-a-date')
  })
})
