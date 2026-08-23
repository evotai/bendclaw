/**
 * UpdateManager — automatic update check scheduler with event emission.
 * Only responsible for checking; does not install.
 *
 * Events:
 *   'update-available' → ReleaseInfo
 *
 * Failures are deliberately not emitted: an unreachable GitHub is not something
 * to nag the user about, and checkForUpdate already persists the reason so
 * `/update` can explain it after the fact. `failureCount` exposes the backoff
 * state for callers that care.
 */

import { EventEmitter } from 'events'
import type { ReleaseInfo } from './types.js'
import { checkForUpdate } from './check.js'

const INITIAL_DELAY = 10_000          // 10s after start
const PERIODIC_CHECK = 30 * 60_000    // 30 min interval
const BACKOFF_RETRY = 60 * 60_000     // probe once an hour after backing off
/**
 * Consecutive failures before the scheduler pauses routine checks. GitHub
 * allows 60 unauthenticated requests/hour/IP, so a shared egress IP can be
 * throttled for reasons this process cannot fix. The pause is bounded: one
 * probe is allowed after BACKOFF_RETRY so a transient outage cannot disable
 * updates for the rest of a long-running session.
 */
const MAX_CONSECUTIVE_FAILURES = 5

export class UpdateManager extends EventEmitter {
  private currentVersion: string
  private now: () => number
  private initialTimer: ReturnType<typeof setTimeout> | null = null
  private periodicTimer: ReturnType<typeof setInterval> | null = null
  private lastNotifiedVersion: string | null = null
  private consecutiveFailures = 0
  private retryAfter = 0
  private inFlight = false
  private stopped = false

  constructor(currentVersion: string, now: () => number = Date.now) {
    super()
    this.currentVersion = currentVersion
    this.now = now
  }

  /** Start the scheduler: delayed first check + periodic checks. */
  start(): void {
    this.initialTimer = setTimeout(() => {
      void this.check()
    }, INITIAL_DELAY)

    this.periodicTimer = setInterval(() => {
      void this.check()
    }, PERIODIC_CHECK)
  }

  /**
   * Run a check. Background checks honour the on-disk TTL, so the periodic
   * timer costs a file read rather than a network round trip most of the time.
   */
  async check(opts?: { force?: boolean }): Promise<void> {
    if (this.stopped || this.inFlight) return

    const force = opts?.force ?? false
    const reachedFailureLimit = this.consecutiveFailures >= MAX_CONSECUTIVE_FAILURES
    if (reachedFailureLimit && !force && this.now() < this.retryAfter) return

    // A scheduled recovery probe must bypass a still-present disk cache or it
    // would report the cached answer as success without testing connectivity.
    const forceNetwork = force || reachedFailureLimit
    this.inFlight = true
    try {
      const result = await checkForUpdate(this.currentVersion, { force: forceNetwork })

      if (result.kind === 'error') {
        this.recordFailure()
        return
      }

      // A stale answer came from cache because the network attempt failed. The
      // user still gets a correct-as-of-last-check result, but the scheduler
      // must count it as a failure or it will never back off.
      if (result.stale) {
        this.recordFailure()
      } else {
        this.consecutiveFailures = 0
        this.retryAfter = 0
      }

      if (result.kind === 'available' && result.latest.version !== this.lastNotifiedVersion) {
        this.lastNotifiedVersion = result.latest.version
        this.emit('update-available', result.latest)
      }
    } catch {
      this.recordFailure()
    } finally {
      this.inFlight = false
    }
  }

  private recordFailure(): void {
    this.consecutiveFailures++
    if (this.consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
      this.retryAfter = this.now() + BACKOFF_RETRY
    }
  }

  /** Consecutive failed checks; resets to 0 on the next success. */
  get failureCount(): number {
    return this.consecutiveFailures
  }

  /** True while routine checks are paused before the next recovery probe. */
  get backedOff(): boolean {
    return this.consecutiveFailures >= MAX_CONSECUTIVE_FAILURES && this.now() < this.retryAfter
  }

  /** Clean up timers. */
  cleanup(): void {
    this.stopped = true
    if (this.initialTimer) {
      clearTimeout(this.initialTimer)
      this.initialTimer = null
    }
    if (this.periodicTimer) {
      clearInterval(this.periodicTimer)
      this.periodicTimer = null
    }
  }
}
