/**
 * Startup tests — verify REPL launches and renders core UI elements.
 */

import { test, expect } from "@microsoft/tui-test";

test("shows banner with evot branding", async ({ terminal }) => {
  await expect(terminal.getByText("evot", { strict: false })).toBeVisible();
});

test("shows model name in banner", async ({ terminal }) => {
  await expect(terminal.getByText("model:", { strict: false })).toBeVisible();
});

test("shows cwd in banner", async ({ terminal }) => {
  await expect(terminal.getByText("cwd:", { strict: false })).toBeVisible();
});

test("shows help hint in banner", async ({ terminal }) => {
  await expect(terminal.getByText("/help commands", { strict: false })).toBeVisible();
});

test("shows prompt input with cursor", async ({ terminal }) => {
  await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
});

test("shows placeholder text when input is empty", async ({ terminal }) => {
  await expect(terminal.getByText("Type a message...", { strict: false })).toBeVisible();
});

test("shows model name in footer", async ({ terminal }) => {
  await expect(terminal.getByText("model:", { strict: false })).toBeVisible();
});

test("startup snapshot", async ({ terminal }) => {
  await expect(terminal.getByText("❯", { strict: false })).toBeVisible();
  await expect(terminal).toMatchSnapshot();
});
