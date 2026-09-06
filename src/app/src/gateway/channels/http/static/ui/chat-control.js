export function isQueuedCommand(text) {
  const token = text.trim().split(/\s+/)[0];
  return /^\/[a-z_]+$/.test(token);
}

/** Run control transport only. A failed steering request is not evidence that
 * the message was rejected: never automatically resend it and risk duplication. */
export async function requestSteering(postJson, sessionId, text) {
  if (!sessionId) return { status: "inactive" };
  try {
    const result = await postJson("/api/chat/steer", { session_id: sessionId, message: text });
    if (!result || typeof result.active !== "boolean") throw new Error("Invalid steering response");
    return { status: result.active ? "queued" : "inactive" };
  } catch (error) {
    return { status: "uncertain", error: String(error?.message || error) };
  }
}
