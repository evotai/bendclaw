import { describe, test, expect } from 'bun:test'
import { parseInput, type KeyEvent } from '../src/term/input.js'

function parse(str: string): KeyEvent[] {
  return parseInput(Buffer.from(str, 'utf-8'))
}

describe('parseInput', () => {
  describe('regular characters', () => {
    test('single character', () => {
      expect(parse('a')).toEqual([{ type: 'char', char: 'a' }])
    })

    test('multiple characters', () => {
      expect(parse('abc')).toEqual([
        { type: 'char', char: 'a' },
        { type: 'char', char: 'b' },
        { type: 'char', char: 'c' },
      ])
    })

    test('space', () => {
      expect(parse(' ')).toEqual([{ type: 'char', char: ' ' }])
    })

    test('unicode character', () => {
      expect(parse('你')).toEqual([{ type: 'char', char: '你' }])
    })

    test('non-BMP character is emitted as one event', () => {
      expect(parse('😀')).toEqual([{ type: 'char', char: '😀' }])
    })
  })

  describe('control characters', () => {
    test('Ctrl+C', () => {
      expect(parse('\x03')).toEqual([{ type: 'ctrl', key: 'c' }])
    })

    test('Ctrl+D', () => {
      expect(parse('\x04')).toEqual([{ type: 'ctrl', key: 'd' }])
    })

    test('Tab', () => {
      expect(parse('\x09')).toEqual([{ type: 'tab' }])
    })

    test('Enter', () => {
      expect(parse('\x0d')).toEqual([{ type: 'enter' }])
    })

    test('Ctrl+L', () => {
      expect(parse('\x0c')).toEqual([{ type: 'ctrl', key: 'l' }])
    })

    test('Ctrl+V', () => {
      expect(parse('\x16')).toEqual([{ type: 'ctrl', key: 'v' }])
    })

    test('Ctrl+W', () => {
      expect(parse('\x17')).toEqual([{ type: 'ctrl', key: 'w' }])
    })

    test('Ctrl+A', () => {
      expect(parse('\x01')).toEqual([{ type: 'ctrl', key: 'a' }])
    })

    // Both of these reach the parser's default `code + 96` branch rather than a
    // case of their own. They are asserted because two keybindings now depend on
    // it: ctrl+b backgrounds a running command, ctrl+g focuses queued prompts.
    // A future explicit case for either code would silently break that.
    test('Ctrl+B, which backgrounds a running command', () => {
      expect(parse('\x02')).toEqual([{ type: 'ctrl', key: 'b' }])
    })

    test('Ctrl+G, which focuses queued prompts', () => {
      expect(parse('\x07')).toEqual([{ type: 'ctrl', key: 'g' }])
    })
  })

  describe('arrow keys', () => {
    test('up', () => {
      expect(parse('\x1b[A')).toEqual([{ type: 'up' }])
    })

    test('down', () => {
      expect(parse('\x1b[B')).toEqual([{ type: 'down' }])
    })

    test('right', () => {
      expect(parse('\x1b[C')).toEqual([{ type: 'right' }])
    })

    test('left', () => {
      expect(parse('\x1b[D')).toEqual([{ type: 'left' }])
    })
  })

  describe('special keys', () => {
    test('home (CSI H)', () => {
      expect(parse('\x1b[H')).toEqual([{ type: 'home' }])
    })

    test('end (CSI F)', () => {
      expect(parse('\x1b[F')).toEqual([{ type: 'end' }])
    })

    test('home (CSI 1~)', () => {
      expect(parse('\x1b[1~')).toEqual([{ type: 'home' }])
    })

    test('end (CSI 4~)', () => {
      expect(parse('\x1b[4~')).toEqual([{ type: 'end' }])
    })

    test('delete (CSI 3~)', () => {
      expect(parse('\x1b[3~')).toEqual([{ type: 'delete' }])
    })

    test('shift-tab (CSI Z)', () => {
      expect(parse('\x1b[Z')).toEqual([{ type: 'shift-tab' }])
    })

    test('page up and page down', () => {
      expect(parse('\x1b[5~')).toEqual([{ type: 'page-up' }])
      expect(parse('\x1b[6~')).toEqual([{ type: 'page-down' }])
    })

    test('backspace (0x7f)', () => {
      expect(parse('\x7f')).toEqual([{ type: 'backspace' }])
    })

    test('escape (bare)', () => {
      expect(parse('\x1b')).toEqual([{ type: 'escape' }])
    })

    test('Alt+Enter (ESC CR)', () => {
      expect(parse('\x1b\r')).toEqual([{ type: 'alt-enter' }])
    })

    test('Shift+Enter (Kitty CSI-u)', () => {
      expect(parse('\x1b[13;2u')).toEqual([{ type: 'shift-enter' }])
    })

    test('Shift+Enter (xterm modifyOtherKeys)', () => {
      expect(parse('\x1b[27;2;13~')).toEqual([{ type: 'shift-enter' }])
    })

    test('Shift+Enter (LF mapping)', () => {
      expect(parse('\n')).toEqual([{ type: 'shift-enter' }])
    })

    test('Shift+Enter (legacy CSI tilde form)', () => {
      expect(parse('\x1b[13;2~')).toEqual([{ type: 'shift-enter' }])
    })

    test('Kitty CSI-u Enter', () => {
      expect(parse('\x1b[13u')).toEqual([{ type: 'enter' }])
    })

    test('Kitty CSI-u Ctrl+C', () => {
      expect(parse('\x1b[99;5u')).toEqual([{ type: 'ctrl', key: 'c' }])
    })

    test('Kitty CSI-u Ctrl+Enter', () => {
      expect(parse('\x1b[13;5u')).toEqual([{ type: 'ctrl-enter' }])
    })

    test('Kitty CSI-u Shift+J/K', () => {
      expect(parse('\x1b[106;2u')).toEqual([{ type: 'shift-char', char: 'j' }])
      expect(parse('\x1b[107;2u')).toEqual([{ type: 'shift-char', char: 'k' }])
    })

  })

  describe('bracketed paste', () => {
    test('parses pasted text', () => {
      const input = '\x1b[200~hello world\x1b[201~'
      expect(parse(input)).toEqual([{ type: 'paste', text: 'hello world' }])
    })

    test('parses multi-line paste', () => {
      const input = '\x1b[200~line1\nline2\nline3\x1b[201~'
      expect(parse(input)).toEqual([{ type: 'paste', text: 'line1\nline2\nline3' }])
    })

    test('paste with special characters', () => {
      const input = '\x1b[200~fn main() { println!("hi"); }\x1b[201~'
      expect(parse(input)).toEqual([{ type: 'paste', text: 'fn main() { println!("hi"); }' }])
    })
  })

  describe('mixed input', () => {
    test('text followed by enter', () => {
      expect(parse('hi\x0d')).toEqual([
        { type: 'char', char: 'h' },
        { type: 'char', char: 'i' },
        { type: 'enter' },
      ])
    })

    test('arrow key followed by character', () => {
      expect(parse('\x1b[Ax')).toEqual([
        { type: 'up' },
        { type: 'char', char: 'x' },
      ])
    })

    test('Ctrl+C followed by text', () => {
      expect(parse('\x03abc')).toEqual([
        { type: 'ctrl', key: 'c' },
        { type: 'char', char: 'a' },
        { type: 'char', char: 'b' },
        { type: 'char', char: 'c' },
      ])
    })
  })

  describe('word / undo key sequences', () => {
    test('legacy alt/ctrl arrows become word moves', () => {
      expect(parse('\x1b[1;3D')).toEqual([{ type: 'word-left' }])
      expect(parse('\x1b[1;5C')).toEqual([{ type: 'word-right' }])
      expect(parse('\x1b[1;3C')).toEqual([{ type: 'word-right' }])
    })

    test('ESC+letter word ops', () => {
      expect(parse('\x1bb')).toEqual([{ type: 'word-left' }])
      expect(parse('\x1bf')).toEqual([{ type: 'word-right' }])
      expect(parse('\x1bd')).toEqual([{ type: 'alt-d' }])
      expect(parse('\x1b\x7f')).toEqual([{ type: 'alt-backspace' }])
    })

    test('Kitty CSI-u alt letter and ctrl+- undo', () => {
      expect(parse('\x1b[98;3u')).toEqual([{ type: 'word-left' }]) // alt+b
      expect(parse('\x1b[102;3u')).toEqual([{ type: 'word-right' }]) // alt+f
      expect(parse('\x1b[100;3u')).toEqual([{ type: 'alt-d' }]) // alt+d
      expect(parse('\x1b[45;5u')).toEqual([{ type: 'undo' }]) // ctrl+-
      expect(parse('\x1f')).toEqual([{ type: 'undo' }]) // ctrl+_
    })

    test('Alt+Delete becomes alt-d', () => {
      expect(parse('\x1b[3;3~')).toEqual([{ type: 'alt-d' }])
    })

    test('Cmd+V asks us to read the clipboard ourselves', () => {
      expect(parse('\x1b[118;9u')).toEqual([{ type: 'paste-clipboard' }])
      expect(parse('\x1b[86;10u')).toEqual([{ type: 'paste-clipboard' }]) // cmd+shift+V
      expect(parse('\x1b[118;9:3u')).toEqual([]) // key release, so one press pastes once
      expect(parse('\x1b[99;9u')).toEqual([]) // other cmd combos stay the terminal's
      expect(parse('\x16')).toEqual([{ type: 'ctrl', key: 'v' }])
      expect(parse('\x1b[118;5u')).toEqual([{ type: 'ctrl', key: 'v' }])
    })
  })
})
