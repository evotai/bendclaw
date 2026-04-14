export function logDirPath(homeDir: string): string {
  return `${homeDir}/.evotai/logs`
}

export function sessionLogPath(homeDir: string, sessionId: string): string {
  return `${logDirPath(homeDir)}/${sessionId}.log`
}

export function sessionTranscriptPath(homeDir: string, sessionId: string): string {
  return `${homeDir}/.evotai/sessions/${sessionId}/transcript.jsonl`
}
