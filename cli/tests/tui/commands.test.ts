/**
 * Slash command tests — verify command UI responses.
 * All outputs are deterministic (no LLM calls).
 */

import { test, expect } from "@microsoft/tui-test";

// ---------------------------------------------------------------------------
// /help
// ---------------------------------------------------------------------------

test.describe("/help", () => {
  test("shows help pane with commands", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/help");
    await expect(terminal.getByText("Keyboard Shortcuts", { strict: false })).toBeVisible();
    await expect(terminal.getByText("Commands", { strict: false })).toBeVisible();
  });

  test("lists visible commands", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/help");
    await expect(terminal.getByText("/plan", { strict: false })).toBeVisible();
    await expect(terminal.getByText("/model", { strict: false })).toBeVisible();
    await expect(terminal.getByText("/resume", { strict: false })).toBeVisible();
  });

  test("shows abbreviation tip", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/help");
    await expect(terminal.getByText("abbreviated", { strict: false })).toBeVisible();
  });

  test("dismissed by Escape", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/help");
    await expect(terminal.getByText("Keyboard Shortcuts", { strict: false })).toBeVisible();
    terminal.keyEscape();
    await expect(
      terminal.getByText("Keyboard Shortcuts", { strict: false })
    ).not.toBeVisible({ timeout: 3000 });
  });

  test("help snapshot", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/help");
    await expect(terminal.getByText("Commands", { strict: false })).toBeVisible();
    await expect(terminal).toMatchSnapshot({ includeColors: true });
  });
});

// ---------------------------------------------------------------------------
// /plan and /act
// ---------------------------------------------------------------------------

test.describe("/plan and /act", () => {
  test("/plan enters planning mode", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/plan");
    await expect(terminal.getByText("Planning mode on", { strict: false })).toBeVisible();
    await expect(terminal.getByText("[plan]", { strict: false })).toBeVisible();
  });

  test("/act returns to action mode", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/plan");
    await expect(terminal.getByText("[plan]", { strict: false })).toBeVisible();
    terminal.submit("/act");
    await expect(terminal.getByText("Action mode on", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// /verbose
// ---------------------------------------------------------------------------

test.describe("/verbose", () => {
  test("toggles verbose mode", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/verbose");
    await expect(terminal.getByText("Verbose mode", { strict: false })).toBeVisible();
  });

  test("/v abbreviation works", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/v");
    await expect(terminal.getByText("Verbose mode", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// /clear
// ---------------------------------------------------------------------------

test.describe("/clear", () => {
  test("clears messages", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/clear");
    await expect(terminal.getByText("Messages cleared", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// /new
// ---------------------------------------------------------------------------

test.describe("/new", () => {
  test("starts new session", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/new");
    await expect(terminal.getByText("New session started", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// /env
// ---------------------------------------------------------------------------

test.describe("/env", () => {
  test("shows variables info", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/env");
    await expect(terminal.getByText("variables", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// unknown command
// ---------------------------------------------------------------------------

test.describe("unknown command", () => {
  test("shows error for unknown command", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/nonexistent");
    await expect(terminal.getByText("Unknown command", { strict: false })).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// ambiguous command
// ---------------------------------------------------------------------------

test.describe("ambiguous command", () => {
  test("shows ambiguous message", async ({ terminal }) => {
    await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
    terminal.submit("/s");
    await expect(terminal.getByText("Ambiguous", { strict: false })).toBeVisible();
  });
});
