import { describe, expect, test } from 'bun:test'
import { decodeConfigInfo } from '../src/native/contracts/config-info.js'
import legacy from './fixtures/contracts/config-info-legacy.json'
import current from './fixtures/contracts/config-info-current.json'

// Frozen shape from native/index.ts at c922f341. Independent of the production
// decoder so a future field removal/type change cannot update both unnoticed.
function legacyReader(json: string): void {
  const value = JSON.parse(json)
  expect(Object.keys(value).sort()).toEqual([
    'availableModels', 'baseUrl', 'envPath', 'hasApiKey', 'protocol', 'provider', 'thinkingLevel',
  ])
  for (const key of ['provider', 'envPath', 'thinkingLevel']) expect(typeof value[key]).toBe('string')
  expect(['anthropic', 'openai', 'openai_responses']).toContain(value.protocol)
  expect(typeof value.hasApiKey).toBe('boolean')
  expect(value.baseUrl === null || typeof value.baseUrl === 'string').toBe(true)
  expect(Array.isArray(value.availableModels)).toBe(true)
  for (const model of value.availableModels) {
    for (const key of ['provider', 'model', 'spec']) expect(typeof model[key]).toBe('string')
    for (const key of Object.keys(model)) {
      expect(['provider', 'protocol', 'model', 'spec', 'group_label', 'group_order', 'sort_order', 'free']).toContain(key)
    }
    if (model.protocol !== undefined) expect(['anthropic', 'openai', 'openai_responses']).toContain(model.protocol)
    if (model.free !== undefined) {
      for (const key of Object.keys(model.free)) expect(['display_name', 'tagline', 'is_new', 'tier']).toContain(key)
    }
  }
}

describe('ConfigInfo boundary', () => {
  test('legacy optional metadata remains absent, not fabricated', () => {
    const decoded = decodeConfigInfo(JSON.stringify(legacy))
    expect(decoded).toEqual(legacy)
    expect(decoded.availableModels[0]?.protocol).toBeUndefined()
  })

  test('current shape round trips through a strict historical reader', () => {
    const json = JSON.stringify(current)
    legacyReader(json)
    expect(decodeConfigInfo(json)).toEqual(current)
  })

  test('empty cloud metadata and nullable base URL are valid', () => {
    expect(decodeConfigInfo(JSON.stringify({ ...legacy, baseUrl: null, availableModels: [{ ...legacy.availableModels[0], free: {} }] })).baseUrl).toBeNull()
  })

  test('unknown additive fields are retained', () => {
    const payload = { ...current, future: true }
    expect(decodeConfigInfo(JSON.stringify(payload))).toEqual(payload)
  })

  test('known malformed fields fail with paths, not raw values', () => {
    for (const [payload, path] of [
      [null, '$'],
      [{ ...current, protocol: 'secret-protocol' }, '$.protocol'],
      [{ ...current, hasApiKey: 'secret-value' }, '$.hasApiKey'],
      [{ ...current, availableModels: {} }, '$.availableModels'],
      [{ ...current, availableModels: [null] }, '$.availableModels[0]'],
      [{ ...current, availableModels: [{ ...current.availableModels[0], sort_order: 'secret' }] }, '$.availableModels[0].sort_order'],
      [{ ...current, availableModels: [{ ...current.availableModels[0], free: { is_new: 'secret' } }] }, '$.availableModels[0].free.is_new'],
    ] as const) {
      expect(() => decodeConfigInfo(JSON.stringify(payload))).toThrow(`Invalid ConfigInfo at ${path}`)
    }
    expect(() => decodeConfigInfo('{"secret-token":')).toThrow('Invalid ConfigInfo at $ (JSON)')
  })

  test('missing required fields fail instead of reaching presentation', () => {
    for (const key of Object.keys(current)) {
      const payload: Record<string, unknown> = { ...current }
      delete payload[key]
      expect(() => decodeConfigInfo(JSON.stringify(payload))).toThrow(`$.${key}`)
    }
  })
})
