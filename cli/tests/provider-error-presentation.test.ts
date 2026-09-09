import { expect, test } from 'bun:test'
import stripAnsi from 'strip-ansi'
import { providerFailurePresentation } from '../src/provider/error-presentation.js'
import { createSpinnerState, formatSpinnerLine, setLongWait, setRetryWait } from '../src/term/spinner.js'

test('provider failure vocabulary is reusable without terminal-specific output', () => {
  for (const [error, label] of [
    ['Network error: tls handshake eof https://private.invalid/key', 'Connection interrupted'],
    ['request timed out', 'Request timed out'],
    ['DNS lookup failed', 'Unable to resolve service address'],
    ['HTTP 529 overloaded', 'Service busy'], ['HTTP 429', 'Rate limited'],
    ['invalid API key', 'Authentication failed'], ['insufficient_quota', 'Quota unavailable'],
  ]) {
    const copy = providerFailurePresentation({ error })
    expect(copy.label).toBe(label)
    expect(copy.label).not.toContain('https:')
    expect(copy.label).not.toContain('\x1b')
  }
  expect(providerFailurePresentation({ kind: 'busy', error: 'tls' }).label).toBe('Service busy')
})

test('input validation numbers are not mistaken for HTTP status codes', () => {
  const error = 'HTTP 400: invalid_request_error: Image has 256 total pixels (16x16), which is below the minimum of 512 pixels.'
  const copy = providerFailurePresentation({ error })
  expect(copy.kind).toBe('invalid-request')
  expect(copy.label).toBe('Invalid request')
  expect(copy.guidance).toContain('attachments')
  expect(copy.guidance).toContain('Retrying unchanged will not help')
  for (const detail of ['minimum of 512 pixels', 'image width 503', 'max 429 tokens', 'size 401 bytes']) {
    expect(providerFailurePresentation({ error: detail }).kind).toBe('unknown')
  }
  for (const message of ['HTTP 503 unavailable', 'HTTP/1.1 502 Bad Gateway', 'status=504', 'status_code: 500', '503 overloaded']) {
    expect(providerFailurePresentation({ error: message }).kind).toBe('busy')
  }
  expect(providerFailurePresentation({ error: 'HTTP 400: connection parameter invalid' }).kind).toBe('invalid-request')
  expect(providerFailurePresentation({ error: 'HTTP 502: invalid_request_error' }).kind).toBe('busy')
})

test('TLS retries keep cumulative elapsed time across the long-wait transition', () => {
  let state = setRetryWait(createSpinnerState(), 2000, 1, 10, 1000, 'tls handshake eof')
  expect(stripAnsi(formatSpinnerLine(state, 1000))).toContain('Connection interrupted · retrying in 2s')
  state = setLongWait(state, 'outage_waiting', 60000, 121000, 'tls handshake eof')
  const text = stripAnsi(formatSpinnerLine(state, 121000, { inputTokens: 1234 }))
  expect(text).toContain('Unable to connect · retrying in 60s')
  expect(text).toContain('waiting 2m')
  expect(text).not.toContain('attempt')
  expect(text).not.toContain('tls')
  expect(text).not.toContain('↑')
  state = setLongWait(state, 'outage_waiting', 60000, 181000, 'tls handshake eof')
  expect(stripAnsi(formatSpinnerLine(state, 181000))).toContain('waiting 3m')
})
