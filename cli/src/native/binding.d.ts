/* auto-generated napi bindings — do not edit */

export interface NapiAgent {
  model: string
  readonly cwd: string
  query(prompt: string, sessionId?: string | null): Promise<NapiQueryStream>
  listSessions(limit?: number | null): Promise<string>
  loadTranscript(sessionId: string): Promise<string>
}

export interface NapiQueryStream {
  readonly sessionId: string
  next(): Promise<string | null>
  abort(): void
}

export declare const NapiAgent: {
  create(model?: string | null): NapiAgent
}

export declare function version(): string
