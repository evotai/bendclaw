import { describe, test, expect } from 'bun:test'
import { maskSecrets } from '../src/utils/secrets.js'

describe('maskSecrets', () => {
  test('masks a single secret', () => {
    expect(maskSecrets('key is sk-abc123xyz', ['sk-abc123xyz']))
      .toBe('key is sk********yz')
  })

  test('masks multiple secrets', () => {
    const result = maskSecrets('a=SECRET1 b=SECRET2', ['SECRET1', 'SECRET2'])
    expect(result).toBe('a=SE***T1 b=SE***T2')
  })

  test('masks longer secrets first', () => {
    const result = maskSecrets('token: abcdef', ['abc', 'abcdef'])
    expect(result).toBe('token: ab**ef')
  })

  test('fully masks short secrets', () => {
    expect(maskSecrets('pw=abc', ['abc'])).toBe('pw=***')
  })

  test('returns text unchanged with no secrets', () => {
    expect(maskSecrets('hello world', [])).toBe('hello world')
  })

  test('handles empty secrets in list', () => {
    expect(maskSecrets('hello', ['', 'hello'])).toBe('*****')
  })

  test('deduplicates secrets', () => {
    expect(maskSecrets('secret', ['secret', 'secret'])).toBe('se**et')
  })
})
