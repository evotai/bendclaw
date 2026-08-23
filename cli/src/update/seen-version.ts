/**
 * Track the last version whose release metadata was handled, so we can show
 * "What's New" once after an update without losing it to a transient outage.
 */

import { join } from 'path'
import { readFileSync, writeFileSync, mkdirSync } from 'fs'
import { isNewer } from './version.js'
import { stateDir } from './paths.js'

function statePath(): string {
  return join(stateDir(), 'last-seen-version.json')
}

interface State {
  version: string
}

function readState(): State | null {
  try {
    const parsed = JSON.parse(readFileSync(statePath(), 'utf-8')) as State
    return typeof parsed?.version === 'string' && parsed.version ? parsed : null
  } catch {
    return null
  }
}

function writeState(state: State): void {
  try {
    mkdirSync(stateDir(), { recursive: true })
    writeFileSync(statePath(), JSON.stringify(state), 'utf-8')
  } catch { /* best effort */ }
}

/**
 * Whether release metadata for the running version still needs to be shown.
 *
 * A missing record is treated as a first install: establish a baseline without
 * showing historical notes. For an upgrade, do not advance the record here —
 * an offline metadata fetch must remain pending for the next startup.
 */
export function releaseNotesPending(currentVersion: string): boolean {
  const state = readState()
  if (!state) {
    writeState({ version: currentVersion })
    return false
  }
  return isNewer(state.version, currentVersion)
}

/**
 * Mark release metadata for a version as handled. Never move the record
 * backwards if another, newer process has already advanced it.
 */
export function markReleaseNotesSeen(version: string): void {
  const state = readState()
  if (!state || isNewer(state.version, version)) {
    writeState({ version })
  }
}
