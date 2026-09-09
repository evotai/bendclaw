/** Customer-facing provider failure vocabulary. No TUI, native, or DOM dependency.
 * Consumers retain the original error separately for diagnostics. Prefer a
 * structured category; string matching is only for historical error events.
 */
export type ProviderFailureKind = 'connection' | 'timeout' | 'dns' | 'busy' | 'rate-limit' | 'quota' | 'authentication' | 'invalid-request' | 'unknown'

export function classifyProviderFailure(error: string): ProviderFailureKind {
  const text = error.toLowerCase()
  // Numeric values in error details (e.g. "minimum of 512 pixels")
  // are not HTTP statuses. Accept explicit markers or a leading bare code.
  const status = Number(text.match(/\bhttp(?:\/\d(?:\.\d)?)?\s+(\d{3})(?!\d)|\bstatus(?:_code|\s+code)?[\s:=]+(\d{3})(?!\d)|^\s*(\d{3})(?:\s|:|$)/)?.slice(1).find(Boolean))
  if (/quota|insufficient_quota|credit balance/.test(text)) return 'quota'
  if (status === 401 || status === 403 || /unauthorized|invalid.api.key|authentication/.test(text)) return 'authentication'
  if (status === 429 || /rate.limit|too many requests/.test(text)) return 'rate-limit'
  if (status >= 500 && status <= 599) return 'busy'
  if (status === 400 || status === 413 || status === 422 || /invalid_request_error/.test(text)) return 'invalid-request'
  if (/dns|enotfound|eai_again|failed to lookup|name resolution/.test(text)) return 'dns'
  if (/timed? ?out|timeout/.test(text)) return 'timeout'
  if (/tls|handshake|connection reset|econnreset|connection refused|econnrefused|network error|connect error/.test(text)) return 'connection'
  if (/overloaded|server.error|service unavailable|bad gateway/.test(text)) return 'busy'
  return 'unknown'
}

const labels: Record<ProviderFailureKind, string> = {
  connection: 'Connection interrupted', timeout: 'Request timed out',
  dns: 'Unable to resolve service address', busy: 'Service busy',
  'rate-limit': 'Rate limited', quota: 'Quota unavailable',
  authentication: 'Authentication failed', 'invalid-request': 'Invalid request', unknown: 'Request failed',
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
      : kind === 'invalid-request'
        ? 'Check the request content and attachments against the model’s input requirements. Retrying unchanged will not help.'
        : undefined,
  }
}
