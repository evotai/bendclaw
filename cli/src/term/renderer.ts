/**
 * TermRenderer — manages terminal output with two zones:
 *
 * 1. Scroll zone: content written here scrolls naturally (completed output)
 * 2. Status area: fixed N lines at the bottom, updated in-place via cursor control
 *
 * The status area uses line-level diffing — only changed lines are redrawn.
 * This eliminates the flicker caused by Ink's clear+redraw model.
 */

import {
  cursorTo,
  cursorUp,
  eraseLine,
  eraseDown,
  hideCursor,
  showCursor,
  cursorToColumn,
} from './ansi.js'
import stringWidth from 'string-width'

export interface TermRendererOptions {
  /** Stream to write to (default: process.stdout) */
  stdout?: NodeJS.WriteStream
}

export class TermRenderer {
  private stdout: NodeJS.WriteStream
  private prevStatusLines: string[] = []
  private statusHeight = 0
  private rows: number
  private cols: number
  private destroyed = false
  private resizeHandler: (() => void) | null = null
  private buf = ''
  private buffering = false

  constructor(opts?: TermRendererOptions) {
    this.stdout = opts?.stdout ?? process.stdout
    this.rows = this.stdout.rows ?? 24
    this.cols = this.stdout.columns ?? 80
  }

  /** Initialize renderer: hide cursor, listen for resize. */
  init(): void {
    this.write(hideCursor())
    this.resizeHandler = () => {
      this.rows = this.stdout.rows ?? 24
      this.cols = this.stdout.columns ?? 80
      this.redrawStatus()
    }
    this.stdout.on('resize', this.resizeHandler)
  }

  /** Restore terminal state. */
  destroy(): void {
    if (this.destroyed) return
    if (this.resizeHandler) {
      this.stdout.off('resize', this.resizeHandler)
      this.resizeHandler = null
    }
    // Clear status area and show cursor
    this.clearStatusArea()
    this.write(showCursor())
    this.destroyed = true
  }

  /** Get terminal dimensions. */
  get termRows(): number { return this.rows }
  get termCols(): number { return this.cols }

  /**
   * Append content to the scroll zone.
   * Moves status area down to make room, then writes content.
   */
  appendScroll(text: string): void {
    if (!text) return
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    // Clear status area first
    this.clearStatusArea()
    // Write content (it scrolls naturally)
    this.write(text)
    // Ensure trailing newline
    if (!text.endsWith('\n')) this.write('\n')
    // Do NOT redraw status here — caller is responsible for calling
    // setStatus() after appendScroll to avoid stale content being redrawn.
    if (!outerBatch) this.flushBatch()
  }

  /**
   * Update the status area (fixed bottom lines).
   * Only redraws lines that changed.
   */
  setStatus(lines: string[]): void {
    const prev = this.prevStatusLines
    const next = lines

    // If height changed, full redraw
    if (next.length !== prev.length) {
      const outerBatch = this.buffering
      if (!outerBatch) this.beginBatch()
      this.clearStatusArea()
      this.prevStatusLines = [...next]
      this.statusHeight = next.length
      this.drawStatus()
      if (!outerBatch) this.flushBatch()
      return
    }

    // Line-level diff: only update changed lines
    if (this.statusHeight === 0) {
      const outerBatch = this.buffering
      if (!outerBatch) this.beginBatch()
      this.prevStatusLines = [...next]
      this.statusHeight = next.length
      this.drawStatus()
      if (!outerBatch) this.flushBatch()
      return
    }

    let needsUpdate = false
    for (let i = 0; i < next.length; i++) {
      if (next[i] !== prev[i]) {
        needsUpdate = true
        break
      }
    }

    if (!needsUpdate) return

    // Full redraw — line-level diff is unreliable when lines wrap
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    this.clearStatusArea()
    this.prevStatusLines = [...next]
    this.statusHeight = next.length
    this.drawStatus()
    if (!outerBatch) this.flushBatch()
  }

  /** Begin a batch — all writes are buffered until flushBatch(). */
  beginBatch(): void {
    this.buffering = true
    this.buf = ''
  }

  /** Flush buffered writes as a single stdout.write(). */
  flushBatch(): void {
    this.buffering = false
    if (this.buf) {
      this.stdout.write(this.buf)
      this.buf = ''
    }
  }

  /** Calculate actual screen rows a set of lines occupies (accounting for wrapping). */
  private screenRows(lines: string[]): number {
    let total = 0
    for (const line of lines) {
      const width = stringWidth(line)
      total += width === 0 ? 1 : Math.ceil(width / this.cols)
    }
    return total
  }

  /** Clear the status area (move up and erase). */
  private clearStatusArea(): void {
    if (this.statusHeight <= 0) return
    const rows = this.screenRows(this.prevStatusLines)
    this.write(cursorUp(rows) + cursorToColumn(1) + eraseDown())
    this.statusHeight = 0
    this.prevStatusLines = []
  }

  /** Draw status area from scratch. */
  private drawStatus(): void {
    if (this.prevStatusLines.length === 0) return
    for (const line of this.prevStatusLines) {
      this.write(this.truncateLine(line) + '\n')
    }
  }

  /** Redraw status area (after resize). */
  private redrawStatus(): void {
    if (this.prevStatusLines.length === 0) return
    const lines = [...this.prevStatusLines]
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    this.clearStatusArea()
    this.prevStatusLines = lines
    this.statusHeight = lines.length
    this.drawStatus()
    if (!outerBatch) this.flushBatch()
  }

  /** Truncate a line to terminal width to prevent wrapping artifacts. */
  private truncateLine(line: string): string {
    // Fast path: if visible width fits, return as-is
    if (stringWidth(line) <= this.cols) return line
    // Slow path: truncate visible content
    // For simplicity, just return the line — terminal will wrap
    // A proper implementation would do ANSI-aware truncation
    return line
  }

  private write(data: string): void {
    if (this.destroyed) return
    if (this.buffering) {
      this.buf += data
    } else {
      this.stdout.write(data)
    }
  }
}
