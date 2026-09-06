import { describe, expect, test } from 'bun:test'
import * as r from '../src/native/contracts/results.js'
import legacy from './fixtures/contracts/native-results-legacy.json'
import { campaignContent } from '../src/term/app/campaigns.js'

describe('native result contracts', () => {
  test('historical results retain missing fields and opaque content', () => {
    for (const [schema, value] of [
      [r.sessionMeta, legacy.session], [r.queuedPrompt, legacy.queue],
      [r.backgroundProcess, legacy.background], [r.compactionOutcome, legacy.compaction],
      [r.cloudUser, legacy.user], [r.loginCode, legacy.login],
    ] as const) {
      expect(schema.read(value, '$')).toBe(value)
    }
    expect(r.decodeResult(JSON.stringify(legacy.session), r.sessionMeta)).not.toHaveProperty('provider')
    const opaque = { ...legacy.queue, message: { future: [null, false, { input: 12 }] }, extension: true }
    expect(r.decodeResult(JSON.stringify(opaque), r.queuedPrompt)).toEqual(opaque)
    expect(r.decodeResult('[{"future_item":{"anything":[false]}}]', r.transcript)).toEqual([{ future_item: { anything: [false] } }])
  })

  test('bad fields fail at their path without private values', () => {
    for (const [schema, value, path] of [
      [r.sessions, [{ ...legacy.session, turns: 'private-token' }], '$[0].turns'],
      [r.queuedPrompt, { ...legacy.queue, version: -1 }, '$.version'],
      [r.backgroundProcess, { ...legacy.background, exit_code: 0.5 }, '$.exit_code'],
      [r.compactionOutcome, { ...legacy.compaction, summary: { secret: 'private-token' } }, '$.summary'],
      [r.variables, [{ key: 'private-token', value: null }], '$[0].value'],
    ] as const) {
      try { schema.read(value, '$'); throw new Error('expected rejection') }
      catch (error) {
        expect(String(error)).toContain(path)
        expect(String(error)).not.toContain('private-token')
      }
    }
    expect(() => r.decodeResult('{"private-token":', r.sessionMeta)).toThrow('Invalid native result at $')
    expect(() => r.decodeResult('[null]', r.transcript)).toThrow('$[0]')
  })

  test('current optional compaction fields and status alternatives are accepted', () => {
    expect(r.compactionOutcome.read({ ...legacy.compaction, method: 'remote', remote_blob_bytes: 12, fallback_reason: 'fixture' }, '$').status).toBe('compacted')
    for (const status of ['nothing_to_compact', 'cancelled']) expect(r.compactionOutcome.read({ status }, '$')).toEqual({ status })
    expect(r.backgroundProcess.read({ ...legacy.background, exit_code: -1 }, '$').exit_code).toBe(-1)
  })

  test('cloud status unions retain extension data and unknown notices are not displayed', () => {
    for (const status of ['pending', 'expired', 'denied']) expect(r.authPoll.read({ status }, '$')).toEqual({ status })
    const success = { status: 'success', state: { user: legacy.user }, models: { future: true } }
    expect(r.authPoll.read(success, '$')).toBe(success)
    expect(r.authRefresh.read({ status: 'login_required', user: null }, '$').user).toBeNull()
    const notices = r.cloudNotices.read([legacy.notice, { ...legacy.notice, kind: 'future' }], '$')
    expect(notices).toHaveLength(2)
    expect(campaignContent(notices)).toHaveLength(1)
  })

  test('native wrappers cannot assert JSON into domain types', async () => {
    for (const file of ['index', 'query-stream']) {
      const source = await Bun.file(new URL(`../src/native/${file}.ts`, import.meta.url)).text()
      expect(source).not.toMatch(/JSON\.parse\([^\n]+\) as /)
      expect(source).not.toMatch(/parseJsonOrThrow\([^\n]+\) as /)
      expect(source).not.toMatch(/:\s*any\b|=\s*any\b/)
    }
  })
})
