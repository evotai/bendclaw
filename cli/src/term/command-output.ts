import type { OutputLine } from '../render/output.js'
import { createProgressLine, type ProgressLine } from './progress-line.js'

export interface CommandOutputPort {
  commitLines: (lines: OutputLine[]) => void
  replaceLine: (id: string, text: string) => boolean
  requestRender: () => void
}

let nextProgressId = 0

/** Shared pre-styled output and one replaceable progress slot per invocation. */
export function createCommandOutput(port: CommandOutputPort, command: string) {
  const commit = (id: string, text: string): void => {
    port.commitLines([{ id, kind: 'system', text, preStyled: true }])
    port.requestRender()
  }
  return {
    commit,
    progress: (): ProgressLine => createProgressLine(`sys-${command}-progress-${nextProgressId++}`, {
      commit,
      replace: (id, text) => {
        const replaced = port.replaceLine(id, text)
        if (replaced) port.requestRender()
        return replaced
      },
    }),
  }
}
