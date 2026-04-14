import { describe, expect, test } from 'bun:test'
import { partitionMessagesForRender } from '../src/utils/renderPartition.js'

describe('partitionMessagesForRender', () => {
  test('caps rendered history and keeps only the recent tail live', () => {
    const messages = ['m1', 'm2', 'm3', 'm4', 'm5', 'm6']
    const result = partitionMessagesForRender(messages, 2, 4)

    expect(result.hiddenCount).toBe(2)
    expect(result.frozen).toEqual(['m3', 'm4'])
    expect(result.live).toEqual(['m5', 'm6'])
  })

  test('keeps all messages live when below the window size', () => {
    const messages = ['m1', 'm2']
    const result = partitionMessagesForRender(messages, 5, 10)

    expect(result.hiddenCount).toBe(0)
    expect(result.frozen).toEqual([])
    expect(result.live).toEqual(['m1', 'm2'])
  })
})
