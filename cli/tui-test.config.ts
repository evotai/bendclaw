import { defineConfig } from "@microsoft/tui-test";
import path from "node:path";

const evotBin =
  process.env.EVOT_BIN ||
  path.resolve(import.meta.dirname, "dist/evot");

export default defineConfig({
  retries: 2,
  trace: true,
  timeout: 15_000,
  testMatch: "tests/tui/**/*.test.ts",
  traceFolder: path.resolve(import.meta.dirname, "tui-traces"),
  use: {
    program: { file: evotBin },
    rows: 40,
    columns: 120,
  },
});
