import type { SessionMeta, SessionWithText } from '../../native/index.js'

export interface ResumeSessionClient {
  listSessions(limit: number): Promise<SessionMeta[]>
  listSessionsWithText(limit: number): Promise<SessionWithText[]>
}

/** Owns preview/full/text snapshots and rejects late results after invalidation.
 * No terminal, editor, selector or rendering dependencies. */
export class ResumeSessionCache {
  private rows: SessionMeta[] | null = null
  private textRows: SessionWithText[] | null = null
  private full = false
  private generation = 0
  private disposed = false
  private previewLoad: Promise<SessionMeta[]> | null = null
  private fullLoad: Promise<SessionMeta[]> | null = null
  private textLoad: Promise<SessionWithText[]> | null = null

  constructor(private readonly client: ResumeSessionClient, private readonly onLoaded: (rows: SessionMeta[]) => void = () => {}) {}

  get metadata(): SessionMeta[] | null { return this.rows }
  get withText(): SessionWithText[] | null { return this.textRows }
  get complete(): boolean { return this.full }

  replace(rows: SessionMeta[], complete = false): void {
    if (this.disposed) return
    this.invalidate()
    this.rows = rows
    this.full = complete
  }

  remove(id: string, fallback: SessionMeta[] = []): void {
    if (this.disposed) return
    const rows = (this.rows ?? fallback).filter(row => row.session_id !== id)
    const text = this.textRows?.filter(row => row.session_id !== id) ?? null
    const complete = this.full
    this.replace(rows, complete)
    this.textRows = text
  }

  invalidate(): void {
    this.generation++
    this.rows = null
    this.textRows = null
    this.full = false
    this.previewLoad = null
    this.fullLoad = null
    this.textLoad = null
  }

  dispose(): void {
    this.disposed = true
    this.invalidate()
  }

  preview(): Promise<SessionMeta[]> {
    if (this.disposed) return Promise.resolve([])
    if (this.rows !== null && (this.full || this.rows.length > 0)) return Promise.resolve(this.rows.slice(0, 20))
    if (this.previewLoad) return this.previewLoad
    const generation = this.generation
    const load = this.client.listSessions(20).then(rows => {
      if (generation !== this.generation) return (this.rows ?? []).slice(0, 20)
      if (!this.full) {
        this.rows = rows
        this.onLoaded(rows)
        return rows
      }
      return (this.rows ?? rows).slice(0, 20)
    })
    this.previewLoad = load
    void load.finally(() => { if (this.previewLoad === load) this.previewLoad = null }).catch(() => {})
    return load
  }

  all(): Promise<SessionMeta[]> {
    if (this.disposed) return Promise.resolve([])
    if (this.full && this.rows !== null) return Promise.resolve(this.rows)
    if (this.fullLoad) return this.fullLoad
    const generation = this.generation
    const load = this.client.listSessions(0).then(rows => {
      if (generation !== this.generation) return this.rows ?? []
      // Full-text rows include all metadata. A slower plain listing must not
      // downgrade the richer snapshot already published in this generation.
      if (this.textRows !== null) return this.textRows
      this.rows = rows
      this.full = true
      this.onLoaded(rows)
      return rows
    })
    this.fullLoad = load
    void load.finally(() => { if (this.fullLoad === load) this.fullLoad = null }).catch(() => {})
    return load
  }

  text(): Promise<SessionWithText[]> {
    if (this.disposed) return Promise.resolve([])
    if (this.textRows !== null) return Promise.resolve(this.textRows)
    if (this.textLoad) return this.textLoad
    const generation = this.generation
    const load = this.client.listSessionsWithText(0).then(rows => {
      if (generation !== this.generation) return this.textRows ?? []
      this.textRows = rows
      this.rows = rows
      this.full = true
      this.onLoaded(rows)
      return rows
    })
    this.textLoad = load
    void load.finally(() => { if (this.textLoad === load) this.textLoad = null }).catch(() => {})
    return load
  }
}
