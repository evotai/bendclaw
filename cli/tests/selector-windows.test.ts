import { describe, expect, test } from 'bun:test'
import { createModelWindow, createResumeWindow } from '../src/term/app/selector-windows.js'
import { SELECTOR_OWNER } from '../src/term/app/selector-identity.js'
import { buildCommandSelectorRegion } from '../src/term/viewmodel/command-selector.js'
import { buildSelectorRegionLines } from '../src/term/viewmodel/selector.js'
import type { ConfigInfo } from '../src/native/contracts/config-info.js'

const config: ConfigInfo = {
  provider: 'test', protocol: 'openai', envPath: '', hasApiKey: true, baseUrl: null, thinkingLevel: '',
  availableModels: [
    { provider: 'test', model: 'a', spec: 'test:a' },
    { provider: 'test', model: 'b', spec: 'test:b' },
  ],
}

describe('selector window composition', () => {
  test('preview and explicit model entry share rows and selection, differing only in focus', () => {
    const preview = createModelWindow(config, 'b')
    const explicit = createModelWindow(config, 'b', true)
    expect(explicit).toEqual({ ...preview, listFocused: true })
    expect(preview.owner).toBe(SELECTOR_OWNER.model)
    expect(preview.items[preview.focusIndex]?.id).toBe('test:b')
    expect(preview.listFocused).toBe(false)
  })

  test('resume keeps cross-workspace items searchable without exposing them initially', () => {
    const items = [{ id: 's1', label: 'Other workspace', searchOnly: true }]
    const state = createResumeWindow(items)
    expect(state.items).toEqual([])
    expect(state.emptyMessage).toContain('No sessions in current cwd')
    expect(createResumeWindow(items, 'Other').items).toHaveLength(1)
  })

  test('slot keeps original content as a suffix and does not shrink on focus', () => {
    const state = createModelWindow(config, 'a')
    for (const [columns, rows] of [[30, 10], [80, 24], [160, 40]]) {
      const preview = buildCommandSelectorRegion(state, columns, rows, false)
      const focused = buildCommandSelectorRegion(state, columns, rows, true)
      expect(preview.length).toBe(focused.length)
      const raw = buildSelectorRegionLines(state, columns, rows, false)
      expect(preview.slice(-raw.length)).toEqual(raw)
      expect(preview.slice(0, -raw.length).every(line => line === '')).toBe(true)
    }
  })
})
