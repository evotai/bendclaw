/**
 * Double Ctrl+C exit handler.
 */

export type InterruptAction = 'clear' | 'show_hint' | 'exit'

export class InterruptHandler {
  private pending = false
  private timer: ReturnType<typeof setTimeout> | null = null

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
    this.timer = setTimeout(() => {
      this.pending = false
    }, 1500)
    return 'show_hint'
  }

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
