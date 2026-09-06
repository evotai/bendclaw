/** Double-press confirmation scoped to an operation, not a terminal session. */
export class InterruptConfirmation {
  private owner: unknown = null
  private deadline = 0
  constructor(private readonly now = Date.now, private readonly windowMs = 5000) {}
  press(owner: unknown): boolean {
    const now = this.now()
    if (this.owner === owner && now < this.deadline) { this.clear(); return true }
    this.owner = owner
    this.deadline = now + this.windowMs
    return false
  }
  clear(): void { this.owner = null; this.deadline = 0 }
}
