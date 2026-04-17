/**
 * Paste accumulator — buffers multi-character stdin chunks that arrive
 * in rapid succession (typical of terminal paste) and flushes them as
 * a single string so shouldCollapse can evaluate the full paste.
 *
 * Single-character input is passed through immediately (normal typing).
 */

const PASTE_FLUSH_TIMEOUT_MS = 80

export class PasteAccumulator {
  private chunks: string[] = []
  private timer: ReturnType<typeof setTimeout> | null = null
  private onFlush: (text: string) => void

  constructor(onFlush: (text: string) => void) {
    this.onFlush = onFlush
  }

  /**
   * Feed input from useInput. Single characters flush immediately;
   * multi-character input (paste) is buffered until no more chunks
   * arrive within the timeout window.
   */
  push(input: string): void {
    if (input.length <= 1) {
      if (input.length === 1) {
        // Normal keystroke — flush immediately
        this.onFlush(input)
      }
      return
    }

    // Multi-char input — likely a paste chunk. Buffer it.
    this.chunks.push(input)
    if (this.timer !== null) {
      clearTimeout(this.timer)
    }
    this.timer = setTimeout(() => {
      this.flush()
    }, PASTE_FLUSH_TIMEOUT_MS)
  }

  /** Whether there are buffered chunks waiting to flush. */
  isPending(): boolean {
    return this.chunks.length > 0
  }

  /** Discard any buffered chunks without flushing. */
  cancel(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer)
      this.timer = null
    }
    this.chunks = []
  }

  private flush(): void {
    this.timer = null
    if (this.chunks.length === 0) return
    const combined = this.chunks.join('')
    this.chunks = []
    this.onFlush(combined)
  }
}
