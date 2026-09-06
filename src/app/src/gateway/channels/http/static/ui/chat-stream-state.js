/** Pure streamed-content state. DOM nodes and animation-frame ownership belong
 * to the renderer; compact markers delimit assistant content generations. */
export class ChatStreamState {
  constructor() { this.buffers = new Map(); this.aborted = false; }
  resetAssistant() { this.buffers.clear(); this.aborted = false; }
  delta(node) {
    const block = node.blocks?.[0];
    if (!block) return false;
    const kind = block.kind === "thinking" ? "thinking" : "text";
    const key = kind + ":" + (node.content_index ?? 0);
    this.buffers.set(key, (this.buffers.get(key) || "") + (block.text || ""));
    return true;
  }
  settle(node) {
    this.buffers.clear();
    if (node.stop_reason === "aborted") this.aborted = true;
  }
  acceptTail(node) {
    const accept = !(this.aborted && node.status === "run");
    if (node.stop_reason === "aborted") this.aborted = true;
    return accept;
  }
}
