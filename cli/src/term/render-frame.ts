/** Data-only boundary between layout producers and terminal backends. */
export interface RenderOverlay {
  lines: string[]
}

export interface RenderFrame {
  lines: string[]
  /** Preserve the trailing edge only after durable content naturally reaches
   * the viewport bottom; this never initially pins a short conversation. */
  bottomAnchor?: boolean
  /** Boundary between committed content and the repaintable live region.
   * Anchored shrinkage is absorbed here rather than above all history. */
  bottomAnchorStart?: number
  /** Transient selector/ask rows can borrow space but cannot establish an
   * anchor. Closing a window must not leave a blank hole behind. */
  transientRows?: number
  /** Stable command-window swaps can retain an anchored viewport even when
   * offscreen history changes. Streaming frames must use normal redraw rules. */
  stableViewport?: boolean
  overlay?: RenderOverlay
}

/** Zero-width cursor hint for IME. Interpreted and stripped by the backend. */
export const CURSOR_MARKER = '\x1b_pi:c\x07'
