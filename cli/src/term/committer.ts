import type { OutputLine } from '../render/output.js'
import { buildOutputBlocks, blocksToLines } from './viewmodel/index.js'

export interface CommitterDeps {
  compactLines: OutputLine[]
  expandedLines: OutputLine[]
  isExpanded: () => boolean
  columns: () => number
  logLines: (lines: string[]) => void
  requestRender: () => void
  /** Called after an in-place edit, so the append-only render cache rebuilds. */
  invalidateHistory: () => void
}

export class Committer {
  constructor(private readonly deps: CommitterDeps) {}

  contextFor(lines: OutputLine[]): { prevKind?: string; columns?: number } {
    const prev = lines.at(-1)
    return { prevKind: prev?.kind, columns: this.deps.columns() }
  }

  restore(lines: OutputLine[], expandedLines: OutputLine[] = lines): void {
    if (lines.length === 0 && expandedLines.length === 0) return
    this.deps.compactLines.push(...lines)
    this.deps.expandedLines.push(...expandedLines)
    this.deps.requestRender()
  }

  commit(lines: OutputLine[]): void {
    if (lines.length === 0) return
    this.deps.compactLines.push(...lines)
    this.deps.expandedLines.push(...lines)
    const visible = this.deps.isExpanded()
      ? this.deps.expandedLines.slice(-lines.length)
      : lines
    this.paint(visible, this.deps.compactLines.slice(0, -lines.length))
  }

  system(id: string, text: string, kind: OutputLine['kind'] = 'system'): void {
    this.commit([{ id, kind, text }])
  }

  /**
   * Commit a line the terminal shows but the screen log never receives.
   *
   * For a revealed secret. The log is a plain file that outlives the session and
   * feeds `/log`, so writing the value there would undo any on-screen erase.
   */
  commitUnlogged(lines: OutputLine[]): void {
    if (lines.length === 0) return
    this.deps.compactLines.push(...lines)
    this.deps.expandedLines.push(...lines)
    this.deps.requestRender()
  }

  /**
   * Rewrite an already-committed line in place, found by id.
   *
   * Scrollback is otherwise append-only, and the terminal redraws each frame
   * from these arrays rather than from what was printed — so replacing the text
   * here does erase it from the screen. Returns false when the line is gone,
   * which is the normal outcome after `/clear`.
   */
  replaceById(id: string, text: string): boolean {
    let found = false
    for (const list of [this.deps.compactLines, this.deps.expandedLines]) {
      const index = list.findIndex((line) => line.id === id)
      if (index === -1) continue
      const existing = list[index]
      if (!existing) continue
      list[index] = { ...existing, text }
      found = true
    }
    if (!found) return false
    // In-place mutation invalidates the cache's append-only prefix.
    this.deps.invalidateHistory()
    this.deps.requestRender()
    return true
  }

  paint(lines: OutputLine[], contextLines: OutputLine[]): void {
    const blocks = buildOutputBlocks(lines, this.contextFor(contextLines))
    this.deps.logLines(blocksToLines(blocks))
    this.deps.requestRender()
  }
}
