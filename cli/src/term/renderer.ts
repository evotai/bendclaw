/**
 * TermRenderer — differential rendering engine for terminal output.
 *
 * Renders a full frame each cycle, diffs against the previous frame, and only
 * redraws changed lines. Uses synchronized output (DEC mode 2026) to eliminate
 * flicker.
 *
 * The cursor/viewport state machine intentionally follows pi-tui's renderer.
 * Keep terminal behavior changes synchronized with:
 *   ~/github/pi/packages/tui/src/tui.ts
 */

import { performance } from 'node:perf_hooks'
import stringWidth from 'string-width'
import stripAnsi from 'strip-ansi'
import { normalizeTerminalOutput, wrapTextWithAnsi, visibleWidth } from '../render/wrap.js'

// --- Constants ---

const MIN_RENDER_INTERVAL_MS = 16
const SYNC_START = '\x1b[?2026h'
const SYNC_END = '\x1b[?2026l'
const CLEAR_LINE = '\x1b[2K'
const CLEAR_VIEWPORT = '\x1b[2J\x1b[H'
const CLEAR_SCREEN_AND_SCROLLBACK = `${CLEAR_VIEWPORT}\x1b[3J`
const SEGMENT_RESET = '\x1b[0m\x1b]8;;\x07'
const OSC133_MARKER = /\x1b\]133;[ABC]\x07/g
const HIDE_CURSOR = '\x1b[?25l'
const SHOW_CURSOR = '\x1b[?25h'
const NOWRAP = '\x1b[?7l'   // Disable auto-wrap (DECAWM off)
const WRAP = '\x1b[?7h'     // Re-enable auto-wrap

// --- Types ---

export interface RenderOverlay {
  lines: string[]
}

export interface RenderFrame {
  lines: string[]
  /**
   * Let a frame that fills the viewport keep its trailing edge on the bottom
   * row, even after the frame shrinks.
   *
   * This is not an initial bottom pin: content decides where the composer sits
   * until the frame first reaches the bottom. After that point, shrinkage gets
   * leading physical space so the trailing edge stays on the row it already
   * reached instead of jumping back to the logical frame's natural position.
   */
  bottomAnchor?: boolean
  /**
   * Start of the repaintable tail that should retain its screen position after
   * a naturally laid-out frame first reaches the viewport bottom. Shrinkage is
   * absorbed here, between committed content and the live region, rather than
   * above the whole frame. Ignored until the frame has actually reached bottom.
   */
  bottomAnchorStart?: number
  /**
   * Rows in this frame that belong to a transient surface: a command window,
   * selector, or ask prompt mounted between the transcript and the composer.
   * They may push the frame past the viewport bottom, but they never establish
   * the bottom anchor. When they close, the composer follows the durable rows
   * back up instead of holding a blank hole open where the surface used to be.
   */
  transientRows?: number
  /**
   * Keep an already anchored, same-height viewport in place when only the
   * logical rows above it also changed. The terminal cannot rewrite scrollback,
   * so those rows remain stale until this transient frame closes; visible rows
   * are still patched normally. Reserved for stable command-window swaps —
   * streaming/history frames must preserve the default full-redraw behavior.
   */
  stableViewport?: boolean
  /** Screen-relative modal content composited over the visible viewport. */
  overlay?: RenderOverlay
}

/** Zero-width marker embedded in rendered output to indicate cursor position for IME. */
export const CURSOR_MARKER = '\x1b_pi:c\x07'

export interface RendererTraceEntry {
  schemaVersion: 1
  ts: string
  kind: 'frame'
  frame: number
  branch: string
  terminal: {
    columns: number
    rows: number
    term?: string
    program?: string
    programVersion?: string
  }
  frameState: {
    previousLines: number
    newLines: number
    maxLinesRenderedBefore: number
    maxLinesRenderedAfter: number
    previousViewportTopBefore: number
    previousViewportTopAfter: number
    hardwareCursorRowBefore: number
    hardwareCursorRowAfter: number
    cursorRow: number | null
    cursorColumn: number | null
    firstChanged: number | null
    lastChanged: number | null
    targetViewportTop: number
    maxVisibleWidth: number
    osc133Markers: number
    overwideLines: Array<{ row: number; width: number }>
  }
  viewportTail?: string[]
  viewportPatch?: { start: number; lines: string[] }
  ansiWrites: string[]
}

export interface TermRendererOptions {
  stdout?: NodeJS.WriteStream
  trace?: (entry: RendererTraceEntry) => void
}

// --- Renderer ---

export class TermRenderer {
  private stdout: NodeJS.WriteStream
  private trace: ((entry: RendererTraceEntry) => void) | null
  private traceWrites: string[] | null = null
  private pendingTraceWrites: string[] = []
  private frameNumber = 0
  private previousLines: string[] = []
  private previousWidth = 0
  private previousHeight = 0
  private hardwareCursorRow = 0
  private maxLinesRendered = 0
  private previousViewportTop = 0
  /** Logical frame length retained after a bottom-anchored tail first reaches the viewport end. */
  private trailingEdgeAnchorLength = 0
  private invalidatedRows = new Set<number>()

  // Render scheduling
  private renderCallback: (() => RenderFrame | string[]) | null = null
  private renderRequested = false
  private renderTimer: ReturnType<typeof setTimeout> | undefined
  private lastRenderAt = 0
  private destroyed = false

  // --- Constructor ---

  constructor(opts?: TermRendererOptions) {
    this.stdout = opts?.stdout ?? process.stdout
    this.trace = opts?.trace ?? null
  }

  // --- Accessors ---

  get termRows(): number {
    return safeDimension(this.stdout.rows, 24)
  }

  get termCols(): number {
    return safeDimension(this.stdout.columns, 80)
  }

  // --- Lifecycle ---

  init(): void {
    this.destroyed = false
    this.write(NOWRAP + HIDE_CURSOR)
    this.stdout.on('resize', this.onResize)
  }

  destroy(): void {
    if (this.renderTimer) {
      clearTimeout(this.renderTimer)
      this.renderTimer = undefined
    }
    // Move cursor below rendered content so shell prompt appears cleanly
    if (this.previousLines.length > 0) {
      const targetRow = this.previousLines.length
      const diff = targetRow - this.hardwareCursorRow
      if (diff > 0) this.write(`\x1b[${diff}B`)
      this.write('\r\n')
    }
    this.write(WRAP + SHOW_CURSOR)
    this.destroyed = true
    this.stdout.off('resize', this.onResize)
  }

  // --- Public API ---

  setRenderCallback(cb: () => RenderFrame | string[]): void {
    this.renderCallback = cb
  }

  /**
   * Force a repaint of every row from `startRow` down on the next frame.
   *
   * A native terminal selection is owned by the terminal, not by us: there is
   * no escape sequence to read or clear it. Rewriting the cells under it is the
   * only way to make it go, and a drag covers a range of rows, so repainting
   * just the caret row would leave the rest of the highlight on screen.
   *
   * Callers pass the first row of the live region. Committed transcript above
   * it is left alone: repainting scrollback on every keystroke would cost more
   * than the stale highlight there is worth.
   */
  invalidateRowsFrom(startRow: number): void {
    if (this.destroyed || this.previousLines.length === 0) return
    const from = Math.max(0, Math.min(startRow, this.previousLines.length - 1))
    for (let row = from; row < this.previousLines.length; row++) {
      this.invalidatedRows.add(row)
    }
    this.requestRender()
  }

  requestRender(force = false): void {
    if (this.destroyed) return
    if (force) {
      this.previousLines = []
      this.previousWidth = -1
      this.previousHeight = -1
      this.hardwareCursorRow = 0
      this.maxLinesRendered = 0
      this.previousViewportTop = 0
      this.trailingEdgeAnchorLength = 0
      this.invalidatedRows.clear()
      if (this.renderTimer) {
        clearTimeout(this.renderTimer)
        this.renderTimer = undefined
      }
      this.renderRequested = true
      process.nextTick(() => {
        if (this.destroyed || !this.renderRequested) return
        this.renderRequested = false
        this.lastRenderAt = performance.now()
        this.doRender()
      })
      return
    }
    if (this.renderRequested) return
    this.renderRequested = true
    process.nextTick(() => this.scheduleRender())
  }

  /**
   * Clear the viewport and scrollback. Used for /clear command.
   */
  clearScreen(): void {
    this.write(SYNC_START + CLEAR_SCREEN_AND_SCROLLBACK + SYNC_END)
    this.previousLines = []
    this.hardwareCursorRow = 0
    this.maxLinesRendered = 0
    this.previousViewportTop = 0
    this.trailingEdgeAnchorLength = 0
    this.invalidatedRows.clear()
    this.previousWidth = this.termCols
    this.previousHeight = this.termRows
  }

  // --- Private: scheduling ---

  private scheduleRender(): void {
    if (this.destroyed || this.renderTimer || !this.renderRequested) return
    const elapsed = performance.now() - this.lastRenderAt
    const delay = Math.max(0, MIN_RENDER_INTERVAL_MS - elapsed)
    this.renderTimer = setTimeout(() => {
      this.renderTimer = undefined
      if (this.destroyed || !this.renderRequested) return
      this.renderRequested = false
      this.lastRenderAt = performance.now()
      this.doRender()
      if (this.renderRequested) this.scheduleRender()
    }, delay)
  }

  private onResize = (): void => {
    // Match pi: resize only schedules a normal frame. doRender compares the
    // actual dimensions and decides whether a full redraw is necessary. Some
    // terminals emit redundant resize events on focus/layout changes.
    this.requestRender()
  }

  // --- Private: core render ---

  private doRender(): void {
    if (this.destroyed || !this.renderCallback) return

    const width = this.termCols
    const height = this.termRows
    const widthChanged = this.previousWidth !== 0 && this.previousWidth !== width
    const heightChanged = this.previousHeight !== 0 && this.previousHeight !== height
    if (this.trace) {
      this.traceWrites = this.pendingTraceWrites
      this.pendingTraceWrites = []
    }
    const frame = ++this.frameNumber
    const previousLineCount = this.previousLines.length
    const maxLinesRenderedBefore = this.maxLinesRendered
    const previousViewportTopBefore = this.previousViewportTop
    const hardwareCursorRowBefore = this.hardwareCursorRow

    // Get new frame from callback
    const raw = this.renderCallback()
    const rendered = Array.isArray(raw) ? { lines: raw } : raw
    let baseLines = rendered.lines
    const anchorStart = rendered.bottomAnchorStart
    const hasTailAnchor = rendered.bottomAnchor
      && anchorStart !== undefined
      && Number.isFinite(anchorStart)
    if (hasTailAnchor) {
      // A resize establishes a new natural layout. Do not carry an anchor from
      // a differently-sized viewport unless the rebuilt frame reaches bottom.
      if (widthChanged || heightChanged) this.trailingEdgeAnchorLength = 0
      // Only durable rows may reach the bottom. A command window that scrolls
      // the frame past the viewport end is not the composer earning its place
      // there; when the window closes the composer must return to the content.
      const transientRows = Math.max(0, Math.trunc(rendered.transientRows ?? 0))
      const durableLength = Math.max(0, baseLines.length - transientRows)
      if (durableLength >= height) {
        this.trailingEdgeAnchorLength = Math.max(this.trailingEdgeAnchorLength, durableLength)
      } else if (this.trailingEdgeAnchorLength > baseLines.length) {
        // Keep committed content in place and absorb transient live-region
        // shrinkage immediately before that region. Padding the frame's top
        // would move history; padding its end would leave the composer high.
        const insertion = Math.max(0, Math.min(Math.trunc(anchorStart), baseLines.length))
        const padding = Array.from(
          { length: this.trailingEdgeAnchorLength - baseLines.length },
          () => '',
        )
        baseLines = [
          ...baseLines.slice(0, insertion),
          ...padding,
          ...baseLines.slice(insertion),
        ]
      }
    }
    // Before the first natural bottom contact, short frames retain their normal
    // flow position. Screen overlays still do their own temporary composition.
    let newLines = rendered.overlay
      ? this.compositeOverlay(baseLines, rendered.overlay, width, height)
      : baseLines
    const cursorPos = this.extractCursorPosition(newLines, height)
    newLines = this.applyLineResets(newLines)

    // Soft overwide-line guard: DECAWM is off, so overflows are silent. When
    // tracing or EVOT_DEBUG=1, surface them so layout bugs are not invisible.
    if (this.trace || process.env.EVOT_DEBUG === '1') {
      for (let row = 0; row < newLines.length; row++) {
        const lineWidth = visibleWidth(newLines[row]!)
        if (lineWidth > width && process.env.EVOT_DEBUG === '1') {
          process.stderr.write(
            `[evot] overwide render line row=${row} width=${lineWidth} > terminal=${width}\n`,
          )
        }
      }
    }

    const traceFrame = (
      branch: string,
      firstChanged: number | null = null,
      lastChanged: number | null = null,
    ): void => {
      if (!this.trace) return
      const viewportTop = Math.max(0, newLines.length - height)
      const viewportTail = newLines.slice(viewportTop)
      const maxVisibleWidth = viewportTail.reduce(
        (max, line) => Math.max(max, stringWidth(stripAnsi(line))),
        0,
      )
      const osc133Markers = viewportTail.reduce(
        (count, line) => count + (line.match(OSC133_MARKER)?.length ?? 0),
        0,
      )
      const overwideLines: Array<{ row: number; width: number }> = []
      for (let row = 0; row < newLines.length; row++) {
        const lineWidth = visibleWidth(newLines[row]!)
        if (lineWidth > width) overwideLines.push({ row, width: lineWidth })
      }
      const differential = branch === 'differential_update'
        || branch === 'deleted_lines_diff'
        || branch === 'no_change'
      const patchStart = firstChanged ?? newLines.length
      const patchEnd = lastChanged === null
        ? patchStart
        : Math.min(lastChanged + 1, newLines.length)
      const entry: RendererTraceEntry = {
        schemaVersion: 1,
        ts: new Date().toISOString(),
        kind: 'frame',
        frame,
        branch,
        terminal: {
          columns: width,
          rows: height,
          term: process.env.TERM,
          program: process.env.TERM_PROGRAM,
          programVersion: process.env.TERM_PROGRAM_VERSION,
        },
        frameState: {
          previousLines: previousLineCount,
          newLines: newLines.length,
          maxLinesRenderedBefore,
          maxLinesRenderedAfter: this.maxLinesRendered,
          previousViewportTopBefore,
          previousViewportTopAfter: this.previousViewportTop,
          hardwareCursorRowBefore,
          hardwareCursorRowAfter: this.hardwareCursorRow,
          cursorRow: cursorPos?.row ?? null,
          cursorColumn: cursorPos?.col ?? null,
          firstChanged,
          lastChanged,
          targetViewportTop: viewportTop,
          maxVisibleWidth,
          osc133Markers,
          overwideLines,
        },
        ...(differential
          ? { viewportPatch: { start: patchStart, lines: newLines.slice(patchStart, patchEnd) } }
          : { viewportTail }),
        ansiWrites: this.traceWrites ?? [],
      }
      this.traceWrites = null
      try {
        this.trace(entry)
      } catch {
        // Diagnostics must never break rendering.
      }
    }

    // --- Full render helper (kept in lockstep with pi-tui) ---
    const fullRender = (clear: boolean, branch: string): void => {
      let buffer = SYNC_START
      if (clear) buffer += CLEAR_SCREEN_AND_SCROLLBACK
      for (let i = 0; i < newLines.length; i++) {
        if (i > 0) buffer += '\r\n'
        buffer += newLines[i]
      }
      buffer += SYNC_END
      this.write(buffer)
      this.hardwareCursorRow = Math.max(0, newLines.length - 1)
      this.maxLinesRendered = clear ? newLines.length : Math.max(this.maxLinesRendered, newLines.length)
      const bufferLength = Math.max(height, newLines.length)
      this.previousViewportTop = Math.max(0, bufferLength - height)
      this.previousLines = newLines
      this.previousWidth = width
      this.previousHeight = height
      this.positionHardwareCursor(cursorPos, newLines.length)
      traceFrame(branch)
    }

    // First render
    if (this.previousLines.length === 0 && !widthChanged && !heightChanged) {
      fullRender(false, 'first_render')
      return
    }

    // Width changed — wrapping changes, must full redraw
    if (widthChanged) {
      fullRender(true, 'width_change')
      return
    }

    // Height changed. Match pi's Termux exception: the software keyboard
    // changes terminal height and replaying history on every toggle is worse.
    if (heightChanged && !isTermuxSession()) {
      fullRender(true, 'height_change')
      return
    }

    // --- Differential render ---
    const previousBufferLength = this.previousHeight > 0
      ? this.previousViewportTop + this.previousHeight
      : height
    let prevViewportTop = heightChanged ? Math.max(0, previousBufferLength - height) : this.previousViewportTop
    let viewportTop = prevViewportTop
    let hardwareCursorRow = this.hardwareCursorRow

    // Re-anchor a shrunk frame. Differential shrink clears the vacated rows but
    // leaves the viewport where the taller frame put it, so the trailing edge
    // stops short of the bottom row by exactly the number of rows lost — an
    // interrupt that discards N rows of thinking lifts the composer N rows.
    // Restoring the invariant used everywhere else (trailing edge on the bottom
    // row) needs the window moved up over the buffer, which cannot be expressed
    // as an in-place patch; the other shrink escape hatches below repaint too.
    if (
      rendered.bottomAnchor
      && newLines.length >= height
      && newLines.length - height < prevViewportTop
    ) {
      fullRender(true, 'bottom_anchor_reanchor')
      return
    }

    const computeLineDiff = (targetRow: number): number => {
      const currentScreenRow = hardwareCursorRow - prevViewportTop
      const targetScreenRow = targetRow - viewportTop
      return targetScreenRow - currentScreenRow
    }

    // Consume explicit row invalidations with this frame. They force a repaint
    // even when the rendered bytes are unchanged (for example, to release a
    // native terminal selection covering the active input row).
    const invalidatedRows = this.invalidatedRows

    // Find first and last changed lines
    let firstChanged = -1
    let lastChanged = -1
    const maxLines = Math.max(newLines.length, this.previousLines.length)
    for (let i = 0; i < maxLines; i++) {
      const oldLine = i < this.previousLines.length ? this.previousLines[i] : ''
      const newLine = i < newLines.length ? newLines[i] : ''
      if (oldLine !== newLine || invalidatedRows.has(i)) {
        if (firstChanged === -1) firstChanged = i
        lastChanged = i
      }
    }
    invalidatedRows.clear()

    const appendedLines = newLines.length > this.previousLines.length
    if (appendedLines) {
      if (firstChanged === -1) firstChanged = this.previousLines.length
      lastChanged = newLines.length - 1
    }
    const appendStart = appendedLines && firstChanged === this.previousLines.length && firstChanged > 0

    // No changes
    if (firstChanged === -1) {
      this.positionHardwareCursor(cursorPos, newLines.length)
      this.previousViewportTop = prevViewportTop
      this.previousHeight = height
      traceFrame('no_change')
      return
    }

    // All changes are in deleted lines (content shrunk)
    if (firstChanged >= newLines.length) {
      if (this.previousLines.length > newLines.length) {
        let buffer = SYNC_START
        // Move to end of new content (clamp to 0 for empty content)
        const targetRow = Math.max(0, newLines.length - 1)
        if (targetRow < prevViewportTop) {
          fullRender(true, 'deleted_lines_above_viewport')
          return
        }
        const lineDiff = computeLineDiff(targetRow)
        if (lineDiff > 0) buffer += `\x1b[${lineDiff}B`
        else if (lineDiff < 0) buffer += `\x1b[${-lineDiff}A`
        buffer += '\r'
        // Clear extra lines without scrolling
        const extraLines = this.previousLines.length - newLines.length
        if (extraLines > height) {
          fullRender(true, 'deleted_lines_exceed_height')
          return
        }
        const clearStartOffset = newLines.length === 0 ? 0 : 1
        if (extraLines > 0 && clearStartOffset > 0) buffer += `\x1b[${clearStartOffset}B`
        for (let i = 0; i < extraLines; i++) {
          buffer += `\r${CLEAR_LINE}`
          if (i < extraLines - 1) buffer += '\x1b[1B'
        }
        const moveBack = Math.max(0, extraLines - 1 + clearStartOffset)
        if (moveBack > 0) buffer += `\x1b[${moveBack}A`
        buffer += SYNC_END
        this.write(buffer)
        this.hardwareCursorRow = targetRow
      }
      this.positionHardwareCursor(cursorPos, newLines.length)
      this.previousLines = newLines
      this.previousWidth = width
      this.previousHeight = height
      this.previousViewportTop = prevViewportTop
      traceFrame('deleted_lines_diff', firstChanged, lastChanged)
      return
    }

    // Differential rendering normally cannot touch rows that were visible only
    // in scrollback, so pi redraws the whole frame. A stable transient viewport
    // is the narrow exception: its logical height and bottom anchor are fixed,
    // and the off-screen rows are deliberately frozen for the lifetime of the
    // surface. Recompute the diff over addressable rows and patch only those.
    if (firstChanged < prevViewportTop) {
      if (
        rendered.stableViewport
        && rendered.bottomAnchor
        && newLines.length === this.previousLines.length
      ) {
        firstChanged = -1
        lastChanged = -1
        for (let i = prevViewportTop; i < newLines.length; i++) {
          if (this.previousLines[i] !== newLines[i]) {
            if (firstChanged === -1) firstChanged = i
            lastChanged = i
          }
        }
        if (firstChanged === -1) {
          this.positionHardwareCursor(cursorPos, newLines.length)
          this.previousLines = newLines
          this.previousWidth = width
          this.previousHeight = height
          this.previousViewportTop = prevViewportTop
          traceFrame('no_change')
          return
        }
      } else {
        fullRender(true, 'off_viewport_redraw')
        return
      }
    }

    // --- Build differential update buffer ---
    let buffer = SYNC_START
    const prevViewportBottom = prevViewportTop + height - 1
    const moveTargetRow = appendStart ? firstChanged - 1 : firstChanged

    // If target is below visible viewport, scroll down. Use a normal hardware
    // scroll (CRLF) so completed output settles into the terminal's real
    // scrollback — selection and scrollback stay consistent. This matches pi's
    // renderer; an in-place repaint here would desync the on-screen window from
    // the terminal's scrollback and make a selection jump on scroll.
    if (moveTargetRow > prevViewportBottom) {
      const currentScreenRow = Math.max(0, Math.min(height - 1, hardwareCursorRow - prevViewportTop))
      const moveToBottom = height - 1 - currentScreenRow
      if (moveToBottom > 0) buffer += `\x1b[${moveToBottom}B`
      const scroll = moveTargetRow - prevViewportBottom
      buffer += '\r\n'.repeat(scroll)
      prevViewportTop += scroll
      viewportTop += scroll
      hardwareCursorRow = moveTargetRow
    }

    // Move cursor to first changed line
    const lineDiff = computeLineDiff(moveTargetRow)
    if (lineDiff > 0) buffer += `\x1b[${lineDiff}B`
    else if (lineDiff < 0) buffer += `\x1b[${-lineDiff}A`

    buffer += appendStart ? '\r\n' : '\r'

    // Render changed lines
    const renderEnd = Math.min(lastChanged, newLines.length - 1)
    for (let i = firstChanged; i <= renderEnd; i++) {
      if (i > firstChanged) buffer += '\r\n'
      buffer += CLEAR_LINE
      buffer += newLines[i]
    }

    // Track where cursor ended up
    let finalCursorRow = renderEnd

    // If content shrunk, clear extra lines
    if (this.previousLines.length > newLines.length) {
      if (renderEnd < newLines.length - 1) {
        const moveDown = newLines.length - 1 - renderEnd
        buffer += `\x1b[${moveDown}B`
        finalCursorRow = newLines.length - 1
      }
      const extraLines = this.previousLines.length - newLines.length
      for (let i = newLines.length; i < this.previousLines.length; i++) {
        buffer += `\r\n${CLEAR_LINE}`
      }
      buffer += `\x1b[${extraLines}A`
    }

    buffer += SYNC_END
    this.write(buffer)

    // Update state
    this.hardwareCursorRow = finalCursorRow
    this.maxLinesRendered = Math.max(this.maxLinesRendered, newLines.length)
    this.previousViewportTop = Math.max(prevViewportTop, finalCursorRow - height + 1)
    this.previousLines = newLines
    this.previousWidth = width
    this.previousHeight = height
    this.positionHardwareCursor(cursorPos, newLines.length)
    traceFrame('differential_update', firstChanged, lastChanged)
  }

  // --- Private: viewport overlays and cursor positioning ---

  /**
   * Composite modal content into the visible viewport without changing the
   * transcript's logical height. This follows pi's screen-relative overlay
   * model: long transcripts keep their length, while short frames are padded to
   * one terminal height only while the overlay is visible.
   */
  private compositeOverlay(
    baseLines: string[],
    overlay: RenderOverlay,
    width: number,
    height: number,
  ): string[] {
    const result = [...baseLines]
    const workingHeight = Math.max(result.length, height)
    while (result.length < workingHeight) result.push('')

    const overlayWidth = Math.max(1, width - Math.min(4, Math.max(0, width - 1)))
    const wrapped = overlay.lines.flatMap(line => wrapTextWithAnsi(line, overlayWidth))
    const maxHeight = Math.max(1, height - Math.min(2, Math.max(0, height - 1)))
    const visibleOverlay = wrapped.slice(0, maxHeight)
    const viewportStart = Math.max(0, workingHeight - height)
    const row = Math.max(0, Math.floor((height - visibleOverlay.length) / 2))

    // Centre the block, not each row. Per-line centring gives every row its own
    // left edge, which shifts rows against each other and destroys any column
    // alignment the overlay content built with padding.
    const blockWidth = visibleOverlay.reduce(
      (max, line) => Math.max(max, stringWidth(stripAnsi(line))),
      0,
    )
    const col = Math.max(0, Math.floor((width - Math.min(blockWidth, width)) / 2))
    for (let index = 0; index < visibleOverlay.length; index++) {
      result[viewportStart + row + index] = `${' '.repeat(col)}${visibleOverlay[index]!}`
    }
    return result
  }

  private applyLineResets(lines: string[]): string[] {
    return lines.map(line => normalizeTerminalOutput(line) + SEGMENT_RESET)
  }

  private extractCursorPosition(lines: string[], height: number): { row: number; col: number } | null {
    const viewportTop = Math.max(0, lines.length - height)
    for (let row = lines.length - 1; row >= viewportTop; row--) {
      const line = lines[row]
      const markerIndex = line.indexOf(CURSOR_MARKER)
      if (markerIndex !== -1) {
        const beforeMarker = line.slice(0, markerIndex)
        const col = stringWidth(stripAnsi(beforeMarker))
        lines[row] = line.slice(0, markerIndex) + line.slice(markerIndex + CURSOR_MARKER.length)
        return { row, col }
      }
    }
    return null
  }

  /**
   * Park the terminal's own cursor on the caret cell but keep it hidden: input
   * methods anchor their preedit window to the reported position, while the
   * prompt paints the visible caret itself.
   */
  private positionHardwareCursor(cursorPos: { row: number; col: number } | null, totalLines: number): void {
    if (!cursorPos || totalLines <= 0) {
      this.write(HIDE_CURSOR)
      return
    }
    const targetRow = Math.max(0, Math.min(cursorPos.row, totalLines - 1))
    const targetCol = Math.max(0, cursorPos.col)
    const rowDelta = targetRow - this.hardwareCursorRow
    let buffer = ''
    if (rowDelta > 0) buffer += `\x1b[${rowDelta}B`
    else if (rowDelta < 0) buffer += `\x1b[${-rowDelta}A`
    buffer += `\x1b[${targetCol + 1}G`
    buffer += HIDE_CURSOR
    this.write(buffer)
    this.hardwareCursorRow = targetRow
  }

  // --- Private: output ---

  private write(data: string): void {
    if (this.destroyed) return
    if (this.trace) {
      if (this.traceWrites) this.traceWrites.push(data)
      else this.pendingTraceWrites.push(data)
    }
    this.stdout.write(data)
  }
}

// --- Helpers ---

function isTermuxSession(): boolean {
  return Boolean(process.env.TERMUX_VERSION)
}

function safeDimension(n: number | undefined, fallback: number): number {
  return n != null && Number.isFinite(n) && n > 0 ? Math.floor(n) : fallback
}
