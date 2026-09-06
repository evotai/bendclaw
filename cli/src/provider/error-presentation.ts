/** Customer-facing provider failure vocabulary. No TUI, native, or DOM dependency.
 * Consumers retain the original error separately for diagnostics. Prefer a
 * structured category; string matching is only for historical error events.
 */
export type ProviderFailureKind = 'connection' | 'timeout' | 'dns' | 'busy' | 'rate-limit' | 'quota' | 'authentication' | 'unknown'

export function classifyProviderFailure(error: string): ProviderFailureKind {
  const text = error.toLowerCase()
  if (/quota|insufficient_quota|credit balance/.test(text)) return 'quota'
  if (/\b(?:401|403)\b|unauthorized|invalid.api.key|authentication/.test(text)) return 'authentication'
  if (/dns|enotfound|eai_again|failed to lookup|name resolution/.test(text)) return 'dns'
  if (/timed? ?out|timeout/.test(text)) return 'timeout'
  if (/tls|handshake|connection reset|econnreset|connection refused|econnrefused|network error|connect error/.test(text)) return 'connection'
  if (/\b429\b|http\s+429|rate.limit|too many requests/.test(text)) return 'rate-limit'
  if (/\b5\d\d\b|overloaded|server.error|service unavailable|bad gateway/.test(text)) return 'busy'
  return 'unknown'
}

const labels: Record<ProviderFailureKind, string> = {
  connection: 'Connection interrupted', timeout: 'Request timed out',
  dns: 'Unable to resolve service address', busy: 'Service busy',
  'rate-limit': 'Rate limited', quota: 'Quota unavailable',
  authentication: 'Authentication failed', unknown: 'Request failed',
}

export function providerFailurePresentation(input: {
  error?: string
  kind?: ProviderFailureKind
  sustained?: boolean
}): { kind: ProviderFailureKind; label: string; guidance?: string } {
  const kind = input.kind ?? classifyProviderFailure(input.error ?? '')
  return {
    kind,
    label: input.sustained && kind === 'connection' ? 'Unable to connect' : labels[kind],
    guidance: kind === 'connection' || kind === 'timeout' || kind === 'dns'
      ? 'Check your network or proxy settings. The service may also be temporarily unavailable.'
      : undefined,
  }
}
