/** HTTP/SSE transport only: no DOM, conversation state or rendering scheduler.
 * The console server emits one JSON object per data line. Keep its tolerant
 * malformed-line handling and final unterminated-line behavior here.
 */
export async function streamChat(payload, { signal, onNode, fetchImpl = fetch }) {
  const response = await fetchImpl("/api/chat", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
    signal,
  });
  if (!response.ok || !response.body) throw new Error("Chat request failed (" + response.status + ")");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let completed = false;
  const consumeLine = (line) => {
    if (!line.startsWith("data: ")) return;
    let node;
    try { node = JSON.parse(line.slice(6)); } catch { return; }
    onNode(node);
  };
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        completed = true;
        break;
      }
      buffer += decoder.decode(value, { stream: true }).replace(/\r/g, "");
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";
      for (const line of lines) consumeLine(line);
    }
    buffer += decoder.decode().replace(/\r/g, "");
    if (buffer) consumeLine(buffer);
  } finally {
    // A consumer/render failure must close the HTTP body too. The server uses
    // receiver closure to abort the run; releaseLock alone leaves it alive.
    if (!completed) {
      try { await reader.cancel(); } catch { /* Preserve the original failure. */ }
    }
    reader.releaseLock();
  }
}
