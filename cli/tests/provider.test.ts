import { describe, expect, test } from 'bun:test'
import { formatModelOptionDetail, formatModelOptionLabel, formatModelLabel, isCloudModel, modelGroupLabel, providerDisplayName, sortModelOptionsForSelector } from '../src/term/app/provider.js'

const options = [
  { provider: 'anthropic', protocol: 'anthropic' as const, model: 'claude-opus-4-8', spec: 'anthropic:claude-opus-4-8' },
  { provider: 'openai', protocol: 'openai_responses' as const, model: 'grok-4.5', spec: 'openai:grok-4.5' },
  { provider: 'droid', protocol: 'openai' as const, model: 'gpt-5.6-sol', spec: 'droid:gpt-5.6-sol' },
  { provider: 'openai', protocol: 'openai_responses' as const, model: 'gpt-5.6-sol', spec: 'openai:gpt-5.6-sol' },
  { provider: 'anthropic', protocol: 'anthropic' as const, model: 'claude-sonnet-5', spec: 'anthropic:claude-sonnet-5' },
]

describe('sortModelOptionsForSelector', () => {
  test('keeps providers contiguous and puts the active provider first', () => {
    const sorted = sortModelOptionsForSelector(options, 'openai:gpt-5.6-sol')

    expect(sorted.map(option => option.spec)).toEqual([
      'openai:gpt-5.6-sol',
      'openai:grok-4.5',
      'anthropic:claude-opus-4-8',
      'anthropic:claude-sonnet-5',
      'droid:gpt-5.6-sol',
    ])
  })

  test('preserves configured order within inactive provider groups', () => {
    const sorted = sortModelOptionsForSelector(options, 'droid:gpt-5.6-sol')

    expect(sorted.map(option => option.spec)).toEqual([
      'droid:gpt-5.6-sol',
      'anthropic:claude-opus-4-8',
      'anthropic:claude-sonnet-5',
      'openai:grok-4.5',
      'openai:gpt-5.6-sol',
    ])
  })

  test('labels each provider group with its wire protocol', () => {
    // Provider and protocol live in the group heading, not on every row.
    expect(modelGroupLabel(options[0]!)).toBe('anthropic · Anthropic Messages')
    expect(modelGroupLabel(options[1]!)).toBe('openai · OpenAI Responses')
    expect(modelGroupLabel(options[2]!)).toBe('droid · OpenAI Chat Completions')
  })
})

// Cloud models carry the group label and order the server pushed; a BYOK
// provider has neither, which is how the two are told apart.
const cloudOptions = [
  { provider: 'droid', protocol: 'openai' as const, model: 'gpt-5.6-sol', spec: 'droid:gpt-5.6-sol' },
  {
    provider: 'evot-pro', protocol: 'anthropic' as const, model: 'anthropic/claude-sonnet-5',
    spec: 'evot-pro:anthropic/claude-sonnet-5',
    group_label: 'Evot Premium', group_order: 1,
    free: { display_name: 'Claude Sonnet 5', tier: 'special' },
  },
  {
    provider: 'evot-free', protocol: 'anthropic' as const, model: 'cohere/north-mini:free',
    spec: 'evot-free:cohere/north-mini:free',
    group_label: 'Evot Free', group_order: 0,
    free: { display_name: 'North Mini', tier: 'base', tagline: 'Fast and free' },
  },
  {
    provider: 'evot-free-openai', protocol: 'openai' as const, model: 'qwen/qwen3:free',
    spec: 'evot-free-openai:qwen/qwen3:free',
    group_label: 'Evot Free', group_order: 0,
    free: { tier: 'base' },
  },
]

describe('cloud model grouping', () => {
  test('cloud tiers sort above BYOK providers, free before pro', () => {
    const sorted = sortModelOptionsForSelector(cloudOptions, 'droid:gpt-5.6-sol')

    expect(sorted.map(option => option.provider)).toEqual([
      'droid',            // active provider stays first
      'evot-free',
      'evot-free-openai',
      'evot-pro',
    ])
  })

  test('the alias is what the picker shows', () => {
    expect(formatModelOptionLabel(cloudOptions[2]!)).toBe('North Mini')
    expect(formatModelOptionLabel(cloudOptions[1]!)).toBe('Claude Sonnet 5')
  })

  test('models without an alias fall back to the real id', () => {
    expect(formatModelOptionLabel(cloudOptions[3]!)).toBe('qwen/qwen3:free')
  })

  test('cloud rows stay bare: the alias already says it', () => {
    // No tier badge or repeated id. A tagline, if set, shows as `(tag)`.
    expect(formatModelOptionDetail(cloudOptions[1]!)).toBe('')
    expect(formatModelOptionDetail(cloudOptions[2]!)).toBe('(Fast and free)')
    expect(formatModelOptionDetail(cloudOptions[3]!)).toBe('')
  })

  test('only NEW earns a detail line', () => {
    const fresh = { ...cloudOptions[2]!, free: { ...cloudOptions[2]!.free, is_new: true } }
    expect(formatModelOptionDetail(fresh)).toBe('(Fast and free) NEW')
  })

  test('a BYOK model keeps its own id as the label', () => {
    expect(formatModelOptionLabel(cloudOptions[0]!)).toBe('gpt-5.6-sol')
    // Its provider and protocol belong to the heading, so the row stays bare.
    expect(formatModelOptionDetail(cloudOptions[0]!)).toBe('')
  })

  test('the group heading is whatever the server pushed', () => {
    expect(modelGroupLabel(cloudOptions[2]!)).toBe('Evot Free')
    expect(modelGroupLabel(cloudOptions[1]!)).toBe('Evot Premium')
    expect(modelGroupLabel(cloudOptions[3]!)).toBe('Evot Free')
  })

  test('protocol halves of a tier share one heading', () => {
    const sorted = sortModelOptionsForSelector(
      [cloudOptions[3]!, cloudOptions[1]!, cloudOptions[2]!],
      'none:none',
    )
    expect(sorted.map(option => modelGroupLabel(option))).toEqual([
      'Evot Free', 'Evot Free', 'Evot Premium',
    ])
  })

  test('a BYOK group heading names the provider and protocol', () => {
    expect(modelGroupLabel(cloudOptions[0]!)).toBe('droid · OpenAI Chat Completions')
  })

  test('cloud membership comes from the server, not the provider name', () => {
    expect(isCloudModel(cloudOptions[2]!)).toBe(true)
    expect(isCloudModel(cloudOptions[0]!)).toBe(false)
    // A provider merely *named* like a cloud tier is not one.
    expect(isCloudModel({
      provider: 'evot-free', model: 'm', spec: 'evot-free:m',
    })).toBe(false)
  })

  test('renaming the tiers server-side reorders the picker', () => {
    // No name is hardcoded, so new labels and orders just work.
    const renamed = [
      { provider: 'tier-b', model: 'b', spec: 'tier-b:b', group_label: 'Premium', group_order: 5 },
      { provider: 'tier-a', model: 'a', spec: 'tier-a:a', group_label: 'Starter', group_order: 2 },
    ]
    const sorted = sortModelOptionsForSelector(renamed, 'none:none')
    expect(sorted.map(o => o.group_label)).toEqual(['Starter', 'Premium'])
  })
})

describe('formatModelLabel', () => {
  test('cloud models show the group heading, not the routing id', () => {
    expect(formatModelLabel('grok-4.6', 'evot-pro-openai', 'Evot Premium'))
      .toBe('grok-4.6@Evot Premium')
  })

  test('BYOK models keep the provider name', () => {
    expect(formatModelLabel('gpt-5.6-sol', 'droid')).toBe('gpt-5.6-sol@droid')
  })

  test('the footer uses the same heading as the picker', () => {
    expect(providerDisplayName(cloudOptions[1], 'evot-pro')).toBe('Evot Premium')
    expect(providerDisplayName(cloudOptions[0], 'droid')).toBe('droid')
  })
})
