import type { ConfigInfo, ModelOption } from '../../native/index.js'
import type { SelectorItem } from '../selector.js'

/** Return configured provider/model pairs, preserving duplicate model ids. */
export function modelOptions(configInfo: ConfigInfo | undefined, fallbackModel: string): ModelOption[] {
  const configured = configInfo?.availableModels ?? []
  if (configured.length > 0) return configured
  const provider = configInfo?.provider ?? ''
  return [{
    provider,
    protocol: configInfo?.protocol,
    model: fallbackModel,
    spec: provider ? `${provider}:${fallbackModel}` : fallbackModel,
  }]
}

/** True when the server pushed this model as part of a cloud group. */
export function isCloudModel(option: ModelOption): boolean {
  return option.group_label !== undefined
}

/** Human name for a wire protocol, or '' when unknown. */
function protocolLabel(protocol: ModelOption['protocol']): string {
  return protocol === 'anthropic'
    ? 'Anthropic Messages'
    : protocol === 'openai'
      ? 'OpenAI Chat Completions'
      : protocol === 'openai_responses'
        ? 'OpenAI Responses'
        : protocol ?? ''
}

/** Heading for a model's group. Cloud groups use the label the server pushed;
 *  BYOK providers are grouped by provider and wire protocol. */
export function modelGroupLabel(option: ModelOption): string {
  if (isCloudModel(option)) return option.group_label?.trim() || option.provider
  return [option.provider, protocolLabel(option.protocol)].filter(Boolean).join(' · ')
}

/** Per-row extra after the name: tagline as `(tag)`, then NEW if the server marked it. */
export function formatModelOptionDetail(option: ModelOption): string {
  const tag = option.free?.tagline?.trim()
  const parts = [
    tag ? `(${tag})` : '',
    isCloudModel(option) && option.free?.is_new ? 'NEW' : '',
  ].filter(Boolean)
  return parts.join(' ')
}

/** Rank a provider: server-ordered cloud groups first, then BYOK providers. */
function cloudRank(option: ModelOption): number {
  return isCloudModel(option) ? (option.group_order ?? 0) : Number.MAX_SAFE_INTEGER
}

export function sortModelOptionsForSelector(options: ModelOption[], activeSpec: string): ModelOption[] {
  // The active-provider boost only applies to BYOK providers; cloud groups
  // must stay contiguous even when the active model sits in a provider that
  // routing split away from the rest of its tier.
  const activeProvider = options.find(option => option.spec === activeSpec && !isCloudModel(option))?.provider
  return options
    .map((option, index) => ({ option, index }))
    .sort((left, right) => {
      const leftGroup = left.option.provider === activeProvider ? 0 : 1
      const rightGroup = right.option.provider === activeProvider ? 0 : 1
      if (leftGroup !== rightGroup) return leftGroup - rightGroup

      // Cloud groups sit above BYOK providers, in the order the server sent.
      // Same heading (Evot Free / Evot Premium) stays contiguous even when
      // routing split the tier into one provider per protocol.
      const cloudOrder = cloudRank(left.option) - cloudRank(right.option)
      if (cloudOrder !== 0) return cloudOrder

      const leftHeading = modelGroupLabel(left.option)
      const rightHeading = modelGroupLabel(right.option)
      const headingOrder = leftHeading.localeCompare(rightHeading)
      if (headingOrder !== 0) return headingOrder

      // Same tier: catalog rank cuts across protocol providers.
      const rankDiff = (right.option.sort_order ?? 0) - (left.option.sort_order ?? 0)
      if (rankDiff !== 0) return rankDiff

      const providerOrder = left.option.provider.localeCompare(right.option.provider)
      if (providerOrder !== 0) return providerOrder

      const leftActive = left.option.spec === activeSpec ? 0 : 1
      const rightActive = right.option.spec === activeSpec ? 0 : 1
      return leftActive - rightActive || left.index - right.index
    })
    .map(entry => entry.option)
}

export function currentModelSpec(configInfo: ConfigInfo | undefined, model: string): string {
  return configInfo?.provider ? `${configInfo.provider}:${model}` : model
}

/** What the picker shows for a model: the server's display name when it has one. */
export function formatModelOptionLabel(option: ModelOption): string {
  const shown = option.free?.display_name?.trim()
  return shown || option.model
}

export function formatModelLabel(model: string, provider: string, groupLabel?: string): string {
  const shown = groupLabel?.trim() || provider
  return shown ? `${model}@${shown}` : model
}

/** Display name for a provider: the server heading when it has one. */
export function providerDisplayName(option: ModelOption | undefined, fallback = ''): string {
  return option?.group_label?.trim() || option?.provider || fallback
}

export function selectModelOption(configInfo: ConfigInfo | undefined, spec: string): ModelOption | undefined {
  return configInfo?.availableModels.find(option => option.spec === spec)
}

/** Rows for the /model overlay: one heading per group, then each model. */
export function modelSelectorItems(options: ModelOption[], activeSpec: string): SelectorItem[] {
  const items: SelectorItem[] = []
  let lastGroup: string | undefined
  for (const option of sortModelOptionsForSelector(options, activeSpec)) {
    const group = modelGroupLabel(option)
    if (group !== lastGroup) {
      items.push({ label: group, header: true, focusable: false, group })
      lastGroup = group
    }
    const detail = formatModelOptionDetail(option)
    const label = formatModelOptionLabel(option)
    items.push({
      label,
      ...(detail ? { detail } : {}),
      id: option.spec,
      group,
      selected: option.spec === activeSpec,
      searchText: `${label} ${option.model} ${option.free?.tagline ?? ''} ${detail} ${option.protocol ?? ''}`,
    })
  }
  return items
}
