/**
 * Input interaction tests — typing, editing, history, tab completion, interrupts.
 */

import { test, expect } from "@microsoft/tui-test";

// ---------------------------------------------------------------------------
// Text input
// ---------------------------------------------------------------------------

test.describe("text input", () => {
  test("typed text appears in input area", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.write("hello world");
    await expect(terminal.getByText("hello world", { strict: false })).toBeVisible();
  });

  test("placeholder disappears after typing", async ({ terminal }) => {
    await expect(terminal.getByText("Type a message...", { strict: false })).toBeVisible();
    terminal.write("x");
    await expect(
      terminal.getByText("Type a message...", { strict: false })
    ).not.toBeVisible({ timeout: 2000 });
  });

  test("backspace deletes character", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.write("helloo");
    await expect(terminal.getByText("helloo", { strict: false })).toBeVisible();
    terminal.keyBackspace();
    await expect(terminal.getByText("helloo", { strict: false })).not.toBeVisible({ timeout: 2000 });
  });
});

// ---------------------------------------------------------------------------
// Ctrl shortcuts
// ---------------------------------------------------------------------------

test.describe("ctrl shortcuts", () => {
  test("Ctrl+L clears input", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.write("some text");
    await expect(terminal.getByText("some text", { strict: false })).toBeVisible();
    terminal.keyPress("l", { ctrl: true });
    await expect(
      terminal.getByText("some text", { strict: false })
    ).not.toBeVisible({ timeout: 2000 });
    await expect(terminal.getByText("Type a message...", { strict: false })).toBeVisible();
  });

  test("Ctrl+A moves to start of line", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.write("hello");
    terminal.keyPress("a", { ctrl: true });
    terminal.write("X");
    await expect(terminal.getByText("Xhello", { strict: false })).toBeVisible();
  });

  test("Ctrl+E moves to end of line", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.write("hello");
    terminal.keyPress("a", { ctrl: true });
    terminal.keyPress("e", { ctrl: true });
    terminal.write("X");
    await expect(terminal.getByText("helloX", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// Tab completion
// ---------------------------------------------------------------------------

test.describe("tab completion", () => {
  test("tab completes slash command", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.write("/he");
    terminal.write("\t");
    await expect(terminal.getByText("/help", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// History navigation
// ---------------------------------------------------------------------------

test.describe("history", () => {
  test("up arrow recalls previous command", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/verbose");
    await expect(terminal.getByText("Verbose mode", { strict: false })).toBeVisible();
    terminal.keyUp();
    await expect(terminal.getByText("/verbose", { strict: false })).toBeVisible();
  });

  test("down arrow after up returns to empty", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/verbose");
    await expect(terminal.getByText("Verbose mode", { strict: false })).toBeVisible();
    terminal.keyUp();
    await expect(terminal.getByText("/verbose", { strict: false })).toBeVisible();
    terminal.keyDown();
    await expect(terminal.getByText("Type a message...", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// Interrupt (Ctrl+C)
// ---------------------------------------------------------------------------

test.describe("interrupt", () => {
  test("Ctrl+C on non-empty input clears it", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.write("some text");
    await expect(terminal.getByText("some text", { strict: false })).toBeVisible();
    terminal.keyCtrlC();
    await expect(terminal.getByText("Type a message...", { strict: false })).toBeVisible();
  });

  test("Ctrl+C on empty input shows exit hint", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.keyCtrlC();
    await expect(
      terminal.getByText("Press Ctrl+C again to exit", { strict: false })
    ).toBeVisible();
  });

  test("double Ctrl+C exits", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.keyCtrlC();
    await expect(
      terminal.getByText("Press Ctrl+C again to exit", { strict: false })
    ).toBeVisible();
    terminal.keyCtrlC();
    await new Promise<void>((resolve) => {
      terminal.onExit(() => resolve());
    });
    expect(terminal.exitResult).toBeDefined();
  });
});
