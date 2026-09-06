/** Conversation operation ownership. No DOM, transport, or render scheduler.
 * A late resume/abort response cannot mutate a newer conversation or run. */
export class ChatState {
  constructor() {
    this.current = null;
    this.generation = 0;
  }
  get streaming() { return this.current !== null; }
  get stopping() { return this.current?.stopping ?? false; }
  get sessionId() { return this.current?.sessionId ?? null; }
  begin(sessionId) {
    if (this.current) return null;
    this.generation++;
    const run = { controller: new AbortController(), sessionId, stopping: false };
    this.current = run;
    return run;
  }
  owns(run) { return this.current === run; }
  bind(sessionId) { if (this.current) this.current.sessionId = sessionId; }
  requestStop() {
    if (!this.current || this.current.stopping) return null;
    this.current.stopping = true;
    return this.current;
  }
  finish(run) {
    if (!this.owns(run)) return false;
    this.current = null;
    return true;
  }
  invalidateNavigation() { this.generation++; }
  beginNavigation() {
    if (this.streaming) return null;
    return ++this.generation;
  }
  ownsNavigation(generation) { return !this.streaming && generation === this.generation; }
  canRestoreSubmission(generation, currentDraft) {
    return generation === this.generation && !currentDraft.trim();
  }
}
