import { describe, expect, test } from 'bun:test'
import {
  isCommandWindowBridge,
  resolveCommandWindowTrigger,
} from '../src/term/app/command-window-trigger.js'

describe('command window trigger', () => {
  test('resolves unique prefixes without opening on text input alone', () => {
    expect(resolveCommandWindowTrigger('/r')).toBe('resume')
    expect(resolveCommandWindowTrigger('/res')).toBe('resume')
    expect(resolveCommandWindowTrigger('/m')).toBe('model')
    expect(resolveCommandWindowTrigger('/mo')).toBe('model')
    expect(resolveCommandWindowTrigger('/sk')).toBe('skill')
    expect(resolveCommandWindowTrigger('/he')).toBe('help')
  })

  test('resolves complete commands and alias prefixes', () => {
    expect(resolveCommandWindowTrigger('/resume')).toBe('resume')
    expect(resolveCommandWindowTrigger('/model')).toBe('model')
    expect(resolveCommandWindowTrigger('/skill')).toBe('skill')
    expect(resolveCommandWindowTrigger('/help')).toBe('help')
    expect(resolveCommandWindowTrigger('/sessions')).toBe('resume')
    expect(resolveCommandWindowTrigger('/sess')).toBe('resume')
  })

  test('recognizes ambiguous bridge prefixes without treating them as triggers', () => {
    expect(isCommandWindowBridge('/')).toBe(true)
    expect(isCommandWindowBridge('/s')).toBe(true)
    expect(isCommandWindowBridge('/m')).toBe(true)
    expect(resolveCommandWindowTrigger('/')).toBeNull()
    expect(resolveCommandWindowTrigger('/s')).toBeNull()
  })

  test('does not bridge empty, unrelated, or argument input', () => {
    expect(isCommandWindowBridge('')).toBe(false)
    expect(isCommandWindowBridge('/n')).toBe(false)
    expect(isCommandWindowBridge('/wat')).toBe(false)
    expect(isCommandWindowBridge('/resume ')).toBe(false)
    expect(isCommandWindowBridge('explain /')).toBe(false)
  })

  test('does not resolve ambiguous, unknown, or non-window commands', () => {
    expect(resolveCommandWindowTrigger('/')).toBeNull()
    expect(resolveCommandWindowTrigger('/s')).toBeNull()
    expect(resolveCommandWindowTrigger('/new')).toBeNull()
    expect(resolveCommandWindowTrigger('/wat')).toBeNull()
  })

  test('does not resolve while entering command arguments', () => {
    expect(resolveCommandWindowTrigger('/resume ')).toBeNull()
    expect(resolveCommandWindowTrigger('/resume renderer')).toBeNull()
    expect(resolveCommandWindowTrigger('/model ')).toBeNull()
    expect(resolveCommandWindowTrigger('/model gpt')).toBeNull()
    expect(resolveCommandWindowTrigger('explain /resume')).toBeNull()
  })
})
