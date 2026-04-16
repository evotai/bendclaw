/* auto-generated napi bindings — do not edit */

export interface NapiAgent {
  model: string
  readonly cwd: string
  query(prompt: string, sessionId?: string | null, toolMode?: string | null): Promise<NapiRun>
  listSessions(limit?: number | null): Promise<string>
  loadTranscript(sessionId: string): Promise<string>
  fork(systemPrompt: string): NapiForkedAgent
  listVariables(): string
  setVariable(key: string, value: string): Promise<void>
  deleteVariable(key: string): Promise<boolean>
  configInfo(): string
  availableModels(): string[]
  setProvider(provider: string): void
  setLimits(maxTurns?: number | null, maxTokens?: number | null, maxDurationSecs?: number | null): void
  appendSystemPrompt(extra: string): void
  addSkillsDirs(dirs: string[]): void
  steer(sessionId: string, text: string): void
  followUp(sessionId: string, text: string): void
  abortRun(sessionId: string): void
}

export interface NapiForkedAgent {
  query(prompt: string): Promise<NapiRun>
}

export interface NapiRun {
  readonly sessionId: string
  next(): Promise<string | null>
  abort(): void
  steer(text: string): void
  followUp(text: string): void
}

export declare const NapiAgent: {
  create(model?: string | null): NapiAgent
}

export declare function version(): string

export declare function startServer(port?: number | null, model?: string | null): Promise<void>
export declare function startServerBackground(port?: number | null, model?: string | null): Promise<number | null>
