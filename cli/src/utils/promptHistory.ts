import type { Key } from 'ink'

export type ArrowAction =
  | 'move_to_line_start'
  | 'move_to_line_end'
  | 'move_up_line'
  | 'move_down_line'
  | 'history_up'
  | 'history_down'

export type HistoryCursorPlacement = 'start' | 'end'

interface ArrowContext {
  linesLength: number
  cursorLine: number
  cursorCol: number
  lineLength?: number
}

export function isHistoryUpShortcut(input: string, key: Key): boolean {
  return key.ctrl === true && input === 'p'
}

export function isHistoryDownShortcut(input: string, key: Key): boolean {
  return key.ctrl === true && input === 'n'
}

export function resolveUpArrowAction(ctx: ArrowContext): ArrowAction {
  if (ctx.linesLength > 1 && ctx.cursorLine > 0) {
    return 'move_up_line'
  }
  if (ctx.cursorCol > 0) {
    return 'move_to_line_start'
  }
  return 'history_up'
}

export function resolveDownArrowAction(ctx: ArrowContext): ArrowAction {
  if (ctx.linesLength > 1 && ctx.cursorLine < ctx.linesLength - 1) {
    return 'move_down_line'
  }
  if (ctx.cursorCol < (ctx.lineLength ?? 0)) {
    return 'move_to_line_end'
  }
  return 'history_down'
}

export function getHistoryCursorPlacement(action: Extract<ArrowAction, 'history_up' | 'history_down'>): HistoryCursorPlacement {
  return action === 'history_up' ? 'start' : 'end'
}
