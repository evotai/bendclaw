export interface ReleaseInfo {
  tag: string       // "v2026.4.13"
  version: string   // "2026.4.13"
  body?: string     // release body markdown
  prerelease?: boolean
}

/** Bookkeeping written after a successful install, mirroring install.sh output. */
export interface InstallState {
  version: string
  target: string
  /** Exact napi binding basename installed for `target`. */
  lib: string[]
  installed_at?: number
}

export type CheckResult =
  | { kind: 'up_to_date'; stale?: boolean }
  | { kind: 'available'; latest: ReleaseInfo; stale?: boolean }
  | { kind: 'error'; message: string }

export type RunResult =
  /**
   * `staleReason` is set when the answer came from cache because the check could
   * not reach GitHub, and carries why. Absent means the answer was confirmed.
   *
   * `proxy` explains which network route the attempt took. It accompanies the
   * outcomes where the route is in question, because the proxy is now chosen
   * automatically: without it, a failure cannot be told apart from one where the
   * user's proxy was never consulted.
   */
  | { kind: 'up_to_date'; staleReason?: string; proxy?: string }
  | { kind: 'updated'; from: string; to: string; notes?: string[] }
  | { kind: 'error'; message: string; proxy?: string }
