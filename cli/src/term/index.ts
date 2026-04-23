export { TermRenderer, type TermRendererOptions } from './renderer.js'
export { parseInput, enableRawMode, type KeyEvent, type KeyHandler } from './input.js'
export {
  createSpinnerState,
  advanceSpinner,
  setSpinnerPhase,
  isSlow,
  formatSpinnerLine,
  type SpinnerState,
  type SpinnerPhase,
} from './spinner.js'
export {
  createSelectorState,
  selectorUp,
  selectorDown,
  selectorSelect,
  selectorType,
  selectorBackspace,
  selectorRemoveItem,
  type SelectorItem,
  type SelectorState,
} from './selector.js'
export {
  createAskState,
  askUp,
  askDown,
  askNextTab,
  askPrevTab,
  askTypeChar,
  askBackspace,
  askClearOther,
  askSelect,
  type AskState,
  type AskQuestion,
  type AskOption,
  type AskAnswer,
} from './ask.js'
export { renderBanner } from './banner.js'
export { startRepl, type ReplOptions } from './repl.js'
export * from './viewmodel/index.js'
export * from './input/editor.js'
export { handleSlashCommand, type CommandContext, type CommandResult } from './app/commands.js'
export { reduceRunEvent, createStreamMachineState, flushStreaming, buildToolStartedLines, buildToolFinishedLines, type StreamMachineState, type StreamUpdate, type StreamContext } from './app/stream.js'
export { askStateToResponse } from './app/ask-user.js'
