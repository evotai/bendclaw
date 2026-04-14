import process from 'node:process'

const TERMINAL_RESTORE_SEQUENCES = [
  '\x1b[?1000l',
  '\x1b[?1002l',
  '\x1b[?1003l',
  '\x1b[?1005l',
  '\x1b[?1006l',
  '\x1b[?1015l',
  '\x1b[?1007l',
  '\x1b[?1049l',
  '\x1b[?1047l',
  '\x1b[?47l',
  '\x1b[?1l',
  '\x1b>',
  '\x1b[?25h',
]

let installed = false
let restored = false

function restoreTerminalState() {
  if (restored || !process.stdout.isTTY) {
    return
  }

  restored = true

  try {
    if (process.stdin.isTTY) {
      process.stdin.setRawMode(false)
    }
  } catch {
    // Ignore stdin cleanup failures during process teardown.
  }

  try {
    process.stdout.write(TERMINAL_RESTORE_SEQUENCES.join(''))
  } catch {
    // Ignore stdout cleanup failures during process teardown.
  }
}

export function installTerminalRestore(): void {
  if (installed) {
    return
  }

  installed = true

  process.once('exit', restoreTerminalState)
  process.once('SIGINT', restoreTerminalState)
  process.once('SIGTERM', restoreTerminalState)
  process.once('uncaughtExceptionMonitor', restoreTerminalState)
  process.once('unhandledRejection', restoreTerminalState)
}

export function restoreTerminalNow(): void {
  restoreTerminalState()
}
