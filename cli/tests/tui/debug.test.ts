/**
 * Debug test — dump terminal content after /help to see what's rendered.
 */

import { test, expect } from "@microsoft/tui-test";

test("debug /help content", async ({ terminal }) => {
  await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
  terminal.submit("/help");
  await expect(terminal.getByText("Commands", { strict: false })).toBeVisible();
  // Dump full buffer
  const buffer = terminal.getBuffer();
  const text = buffer.map((row) => row.join("").trimEnd()).filter(l => l.length > 0).join("\n");
  console.log("=== TERMINAL CONTENT ===");
  console.log(text);
  console.log("=== END ===");
  expect(true).toBe(true);
});
