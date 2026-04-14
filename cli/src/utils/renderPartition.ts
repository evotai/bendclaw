export function partitionMessagesForRender<T>(
  messages: T[],
  liveWindowSize: number,
  maxRenderedMessages = liveWindowSize,
): { hiddenCount: number; frozen: T[]; live: T[] } {
  const safeMaxRendered = Math.max(maxRenderedMessages, liveWindowSize)
  const hiddenCount = Math.max(0, messages.length - safeMaxRendered)
  const visibleMessages = hiddenCount > 0 ? messages.slice(hiddenCount) : messages

  if (liveWindowSize <= 0 || visibleMessages.length <= liveWindowSize) {
    return { hiddenCount, frozen: [], live: visibleMessages }
  }

  const splitAt = visibleMessages.length - liveWindowSize
  return {
    hiddenCount,
    frozen: visibleMessages.slice(0, splitAt),
    live: visibleMessages.slice(splitAt),
  }
}
