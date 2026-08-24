/**
 * Proxy auto-selection for update traffic.
 *
 * `evot update` spans two runtimes with different proxy behaviour, so relying on
 * ambient environment variables silently updates through only one of them:
 *
 *   - Bun's `fetch` (release list, install.sh) honours `HTTPS_PROXY` /
 *     `https_proxy` for https URLs but ignores `ALL_PROXY` entirely, and rejects
 *     a `socks5://` value with `UnsupportedProxyProtocol`.
 *   - `install.sh` runs curl/wget, which honour all four variables and support
 *     socks5.
 *
 * A machine configured only with `all_proxy=socks5://...` therefore proxies the
 * 37 MB release asset but fetches the release list directly. This module picks
 * one proxy for both halves, verifies it accepts connections, and hands each
 * runtime a form it understands.
 *
 * Discovery is delegated rather than reimplemented:
 *   - `proxy-from-env` resolves the `*_PROXY` / `NO_PROXY` variables. Its
 *     precedence and matching rules are derived from curl, wget and Python, so
 *     evot agrees with the tools users already configure for.
 *   - `mac-system-proxy` reads and parses `scutil --proxy`, covering a
 *     system-wide proxy set with no environment variables at all.
 *
 * What remains here is only what those libraries do not answer: whether a
 * configured proxy is actually listening, which runtime can use its scheme, and
 * how to pass the decision to a child process.
 */

import { getProxyForUrl } from 'proxy-from-env'
import { getMacSystemProxy, type MacProxySettings } from 'mac-system-proxy'

/** Budget for the reachability probe. A local proxy answers in single-digit ms. */
const PROBE_TIMEOUT_MS = 1_500

/** Schemes Bun's `fetch` can proxy through. Everything else is curl-only. */
const FETCH_SCHEMES = new Set(['http', 'https'])

/**
 * Endpoints an update contacts: release list, installer, release asset.
 *
 * Full URLs rather than hostnames because `NO_PROXY` matching is scheme- and
 * port-sensitive, and `getProxyForUrl` needs both to apply its rules.
 */
export const UPDATE_URLS = [
  'https://raw.githubusercontent.com/',
  'https://github.com/',
]

export interface ProxyCandidate {
  /** Normalized proxy URL, credentials preserved. */
  url: string
  /** Where the value came from, for diagnostics. */
  source: string
  scheme: string
  host: string
  port: number
}

export interface ProxySelection {
  /** Proxy for Bun `fetch` calls, or null to connect directly. */
  fetchProxy: ProxyCandidate | null
  /** Proxy for the install.sh subprocess (curl/wget), or null for direct. */
  shellProxy: ProxyCandidate | null
  /** One-line explanation of the decision. */
  reason: string
}

export type ProxyEnv = Record<string, string | undefined>

/** Proxy variables cleared before injecting a decision, so no stale value leaks. */
const SHELL_PROXY_KEYS = [
  'http_proxy',
  'HTTP_PROXY',
  'https_proxy',
  'HTTPS_PROXY',
  'all_proxy',
  'ALL_PROXY',
]

const DEFAULT_PORTS: Record<string, number> = {
  http: 80,
  https: 443,
  socks5: 1080,
  socks5h: 1080,
  socks4: 1080,
}

/**
 * Normalize a proxy value into a candidate.
 *
 * `proxy-from-env` guarantees a scheme on its output, but `EVOT_PROXY` and the
 * macOS settings are raw, and a bare `host:port` is a common way to write both.
 */
export function parseProxyUrl(raw: string, source: string): ProxyCandidate | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  // `off` / `none` / `0` are how users disable a proxy without unsetting it.
  if (/^(off|none|false|0)$/i.test(trimmed)) return null

  const withScheme = trimmed.includes('://') ? trimmed : `http://${trimmed}`
  let parsed: URL
  try {
    parsed = new URL(withScheme)
  } catch {
    return null
  }

  const scheme = parsed.protocol.replace(/:$/, '').toLowerCase()
  if (!(scheme in DEFAULT_PORTS)) return null
  if (!parsed.hostname) return null

  const port = parsed.port ? Number(parsed.port) : DEFAULT_PORTS[scheme]
  if (!Number.isInteger(port) || port <= 0 || port > 65535) return null

  const auth = parsed.username
    ? `${parsed.username}${parsed.password ? `:${parsed.password}` : ''}@`
    : ''
  return {
    url: `${scheme}://${auth}${parsed.hostname}:${port}`,
    source,
    scheme,
    host: parsed.hostname,
    port,
  }
}

/**
 * Proxies named by the environment, via `proxy-from-env`.
 *
 * Resolution is per-URL because `NO_PROXY` can exempt one endpoint and not
 * another, and because the scheme selects which variable applies. Asking for
 * every update URL and deduplicating is what makes "GitHub is exempt" and
 * "GitHub goes through X" both expressible.
 */
export function envCandidates(
  env: ProxyEnv = process.env,
  urls: string[] = UPDATE_URLS,
): ProxyCandidate[] {
  const found: ProxyCandidate[] = []
  // getProxyForUrl reads process.env directly, so a caller-supplied env has to
  // be installed for the duration of the lookup.
  withEnv(env, () => {
    for (const url of urls) {
      const resolved = getProxyForUrl(url)
      if (!resolved) continue
      const candidate = parseProxyUrl(resolved, 'environment')
      if (candidate) found.push(candidate)
    }
  })
  return found
}

/**
 * True when `NO_PROXY` is what stopped an otherwise-applicable proxy.
 *
 * Both "nothing configured" and "exempted" produce no candidates, and only the
 * second is a deliberate choice worth reporting. The test is a counterfactual:
 * resolve again with the exemption list removed, and if a proxy appears then
 * `NO_PROXY` is the reason it did not the first time. Checking merely that no
 * proxy resolved would also catch an `http_proxy` that never applied to https
 * URLs in the first place, which is an unrelated situation.
 */
export function envExemptsAll(
  env: ProxyEnv = process.env,
  urls: string[] = UPDATE_URLS,
): boolean {
  const exemptionList = (env.no_proxy ?? env.NO_PROXY ?? '').trim()
  if (!exemptionList) return false

  const withExemption = envCandidates(env, urls)
  if (withExemption.length > 0) return false

  const without: ProxyEnv = { ...env }
  delete without.no_proxy
  delete without.NO_PROXY
  return envCandidates(without, urls).length > 0
}

/** Run `fn` with `env` installed as `process.env`, then restore it. */
function withEnv<T>(env: ProxyEnv, fn: () => T): T {
  if (env === process.env) return fn()
  const saved = process.env
  // Bun's process.env is a live binding to the real environment; replacing the
  // object wholesale is how a caller-supplied env gets seen by the library.
  process.env = env as NodeJS.ProcessEnv
  try {
    return fn()
  } finally {
    process.env = saved
  }
}

/**
 * Proxies configured system-wide, currently macOS only.
 *
 * This is the case behind "it only works with the system proxy switched on": no
 * variable is exported, so every env lookup finds nothing and the update goes
 * direct. `mac-system-proxy` owns running and parsing `scutil --proxy`.
 *
 * Linux has no comparable single source of truth (GNOME, KDE and plain shells
 * all differ), so it stays env-only there.
 */
export async function systemCandidates(
  read: () => Promise<MacProxySettings> = getMacSystemProxy,
): Promise<ProxyCandidate[]> {
  if (process.platform !== 'darwin') return []

  let settings: MacProxySettings
  try {
    settings = await read()
  } catch {
    // No system proxy configured, or scutil unavailable. Not an error.
    return []
  }

  // Ordered like the env variables: https-specific first, socks last because
  // only curl can use it.
  const tiers: Array<{ on?: string; host?: string; port?: string; scheme: string; label: string }> = [
    {
      on: settings.HTTPSEnable,
      host: settings.HTTPSProxy,
      port: settings.HTTPSPort,
      // macOS records an HTTPS proxy as host+port; the hop itself is a plain
      // HTTP CONNECT, so the scheme is http rather than https.
      scheme: 'http',
      label: 'system:https',
    },
    {
      on: settings.HTTPEnable,
      host: settings.HTTPProxy,
      port: settings.HTTPPort,
      scheme: 'http',
      label: 'system:http',
    },
    {
      on: settings.SOCKSEnable,
      host: settings.SOCKSProxy,
      port: settings.SOCKSPort,
      scheme: 'socks5',
      label: 'system:socks',
    },
  ]

  const candidates: ProxyCandidate[] = []
  for (const tier of tiers) {
    if (tier.on !== '1' || !tier.host || !tier.port) continue
    const candidate = parseProxyUrl(`${tier.scheme}://${tier.host}:${tier.port}`, tier.label)
    if (candidate) candidates.push(candidate)
  }
  return candidates
}

/**
 * Every configured proxy, best first, deduplicated by URL.
 *
 * `EVOT_PROXY` is the explicit override and answers on its own, including when
 * it disables proxying. Otherwise environment values win over system settings,
 * since an exported variable is the more deliberate signal.
 */
export async function collectCandidates(opts: {
  env?: ProxyEnv
  urls?: string[]
  system?: () => Promise<ProxyCandidate[]>
} = {}): Promise<ProxyCandidate[]> {
  const env = opts.env ?? process.env
  const override = env.EVOT_PROXY?.trim()
  if (override) {
    const candidate = parseProxyUrl(override, 'EVOT_PROXY')
    return candidate ? [candidate] : []
  }

  const found = [
    ...envCandidates(env, opts.urls),
    ...(await (opts.system ?? systemCandidates)()),
  ]

  const seen = new Set<string>()
  return found.filter((candidate) => {
    if (seen.has(candidate.url)) return false
    seen.add(candidate.url)
    return true
  })
}

/**
 * TCP reachability check.
 *
 * A configured-but-dead proxy is the common failure after a VPN client exits:
 * the variables survive in the shell while nothing listens. Probing turns that
 * from a stalled update into a direct connection.
 */
export async function probeReachable(
  host: string,
  port: number,
  timeoutMs: number = PROBE_TIMEOUT_MS,
): Promise<boolean> {
  return new Promise((resolve) => {
    let settled = false
    const finish = (reachable: boolean): void => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve(reachable)
    }
    const timer = setTimeout(() => finish(false), timeoutMs)

    Bun.connect({
      hostname: host,
      port,
      socket: {
        open(socket) {
          // Settle before closing: `socket.end()` invokes the close handler
          // synchronously, so releasing the socket first would let the failure
          // path win a connection that actually succeeded.
          finish(true)
          socket.end()
        },
        data() { /* nothing is sent; the handshake alone is the signal */ },
        // A refused port rejects the connect promise rather than reaching here,
        // so this only fires for a teardown that preempted `open`.
        close() { finish(false) },
        error() { finish(false) },
      },
    }).catch(() => finish(false))
  })
}

/**
 * Pick the proxy for both halves of an update.
 *
 * Candidates are tried in precedence order and the first reachable one wins for
 * curl. Bun's `fetch` gets the same proxy only when it can speak that scheme;
 * for a socks5 proxy the two halves deliberately diverge (curl proxies, fetch
 * goes direct) because that beats failing the release-list call outright.
 *
 * Everything degrades to a direct connection: a proxy that cannot be reached
 * must not be able to break an update that would have worked without it.
 */
export async function selectProxy(opts: {
  env?: ProxyEnv
  urls?: string[]
  probe?: (host: string, port: number) => Promise<boolean>
  candidates?: ProxyCandidate[]
  system?: () => Promise<ProxyCandidate[]>
} = {}): Promise<ProxySelection> {
  const env = opts.env ?? process.env
  const probe = opts.probe ?? ((host, port) => probeReachable(host, port))
  const candidates = opts.candidates
    ?? await collectCandidates({ env, urls: opts.urls, system: opts.system })

  if (candidates.length === 0) {
    // Separate a deliberate exemption from nothing being configured at all.
    if (envExemptsAll(env, opts.urls)) {
      return {
        fetchProxy: null,
        shellProxy: null,
        reason: 'NO_PROXY covers GitHub; connecting directly',
      }
    }
    return {
      fetchProxy: null,
      shellProxy: null,
      reason: 'no proxy configured; connecting directly',
    }
  }

  const rejected: string[] = []
  for (const candidate of candidates) {
    if (!(await probe(candidate.host, candidate.port))) {
      rejected.push(`${candidate.source} (${candidate.url}) unreachable`)
      continue
    }

    const usableByFetch = FETCH_SCHEMES.has(candidate.scheme)
    const skipped = rejected.length > 0 ? `; skipped ${rejected.join(', ')}` : ''
    return {
      fetchProxy: usableByFetch ? candidate : null,
      shellProxy: candidate,
      reason: usableByFetch
        ? `using proxy ${candidate.url} from ${candidate.source}${skipped}`
        : `using proxy ${candidate.url} from ${candidate.source} for the download only `
          + `(${candidate.scheme} is not supported by the runtime's fetch)${skipped}`,
    }
  }

  return {
    fetchProxy: null,
    shellProxy: null,
    reason: `connecting directly; ${rejected.join(', ')}`,
  }
}

/**
 * Apply a selection to the environment handed to install.sh.
 *
 * Existing proxy variables are cleared first: leaving a rejected value in place
 * would let curl use a proxy this module already found unreachable, and leaving
 * a socks-only `all_proxy` beside a chosen http proxy makes the effective route
 * depend on curl's own precedence rather than the decision made here.
 *
 * `NO_PROXY` is deliberately preserved — it is the user's exemption list, and a
 * selection only exists when it did not already cover GitHub.
 */
export function applyProxyToEnv(
  env: Record<string, string>,
  selection: ProxySelection,
): Record<string, string> {
  const result = { ...env }
  for (const key of SHELL_PROXY_KEYS) delete result[key]

  const proxy = selection.shellProxy
  if (!proxy) return result

  // Both are set: curl consults the scheme-specific variable per request, and
  // an https release URL can redirect through an http mirror.
  result.https_proxy = proxy.url
  result.HTTPS_PROXY = proxy.url
  result.http_proxy = proxy.url
  result.HTTP_PROXY = proxy.url
  return result
}

let cached: Promise<ProxySelection> | null = null

/**
 * Process-wide proxy decision, resolved once.
 *
 * The scheduler checks for updates every 30 minutes for the life of a session,
 * and each check would otherwise re-probe. Proxy configuration cannot change
 * mid-process anyway: `process.env` was snapshotted at spawn, so exporting a
 * variable in another shell has no effect on a running TUI.
 */
export function resolveUpdateProxy(): Promise<ProxySelection> {
  cached ??= selectProxy()
  return cached
}

/** Drop the memoized decision. For tests. */
export function resetUpdateProxy(): void {
  cached = null
}
