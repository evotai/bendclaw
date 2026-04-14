export function renderStreamingText(text: string): string {
  return text
}

export function shouldAnimateTerminalTitle(): boolean {
  return process.env.EVOT_ANIMATE_TITLE === '1'
}
