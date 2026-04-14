/**
 * Double Ctrl+C exit handler — ported from Rust interrupt.rs.
 * First Ctrl+C on empty input shows a hint, second exits.
 */

export type InterruptAction = 'clear' | 'show_hint' | 'exit'

export class InterruptHandler {
  private pending = false
  private timer: ReturnType<typeof setTimeout> | null = null

  /**
   * Called when Ctrl+C is pressed.
   * @param lineEmpty whether the input line is empty
   */
  onInterrupt(lineEmpty: boolean): InterruptAction {
    if (!lineEmpty) {
      this.reset()
      return 'clear'
    }
    if (this.pending) {
      this.reset()
      return 'exit'
    }
    this.pending = true
    // Auto-reset after 1.5 seconds
    this.timer = setTimeout(() => {
      this.pending = false
    }, 1500)
    return 'show_hint'
  }

  /** Called on any normal input — cancels pending exit. */
  onInput(): void {
    this.reset()
  }

  private reset(): void {
    this.pending = false
    if (this.timer) {
      clearTimeout(this.timer)
      this.timer = null
    }
  }
}
