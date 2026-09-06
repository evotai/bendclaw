import { expect, test } from 'bun:test'
import { RunInteraction, resolveRunInteraction, type RunInteractionInput } from '../src/term/app/run-interaction.js'
import { runStatusPresentation } from '../src/term/viewmodel/run-status.js'
import { decideReplControl } from '../src/term/app/repl-control.js'
import { createEditorState } from '../src/term/input/editor.js'

const owner = {}
const cases: Array<[RunInteractionInput, string, boolean, boolean]> = [
  [{ active: false, owner: null }, 'idle', false, false],
  [{ active: false, owner: null, backgroundTasks: 1 }, 'waiting-background', false, false],
  [{ active: true, owner }, 'active', true, true],
  [{ active: true, owner, foregroundTasks: 1 }, 'active', true, true],
  [{ active: true, owner, blockingWaits: 1 }, 'waiting-task', true, false],
  [{ active: true, owner, phase: 'retrying' }, 'retrying', true, false],
  [{ active: true, owner, localOperation: true }, 'local-operation', false, false],
  [{ active: true, owner, compacting: true }, 'compacting', true, false],
  [{ active: true, owner: null }, 'active', false, true],
]

test('status hints and key routing share the same capabilities for every state', () => {
  for (const [input, kind, interruptible, usage] of cases) {
    const interaction = resolveRunInteraction(input)
    const status = runStatusPresentation(interaction)
    expect(interaction.kind).toBe(kind)
    expect(status.showUsage).toBe(usage)
    expect(status.hint.includes('esc twice')).toBe(interruptible && interaction.backgroundAction === null)
    expect(status.hint.includes('to background') || status.hint.includes('to release wait')).toBe(interaction.backgroundAction !== null)
    const route = (event: { type: 'escape' } | { type: 'ctrl'; key: string }) => decideReplControl({
      event, interaction, overlay: { kind: 'none' }, editor: createEditorState(),
      isLoading: input.active, hasStream: Boolean(input.owner), exitHint: false,
      logMode: false, hasQueuedPrompt: false,
    }).map(action => action.kind)
    expect(route({ type: 'escape' }).includes('interrupt')).toBe(interruptible)
    expect(route({ type: 'ctrl', key: 'b' }).includes('reclaim-turn')).toBe(interaction.backgroundAction !== null)
  }
})

test('two Esc presses belong to one operation and expire after five seconds', () => {
  let now = 0
  const controller = new RunInteraction(() => now)
  const run = { active: true, owner, blockingWaits: 1 }
  expect(controller.requestInterrupt(run)).toBe('confirm')
  expect(runStatusPresentation(controller.snapshot(run)).hint).toBe(' · esc again to interrupt')
  expect(controller.requestInterrupt(run)).toBe('interrupt')
  expect(controller.snapshot(run).interruptPending).toBe(false)
  expect(controller.requestInterrupt(run)).toBe('confirm')
  now = 5000
  expect(controller.requestInterrupt(run)).toBe('confirm')
  expect(controller.requestInterrupt({ ...run, owner: {} })).toBe('confirm')
  controller.clear()
  expect(controller.snapshot(run).interruptPending).toBe(false)
  expect(controller.requestInterrupt({ active: false, owner: null, backgroundTasks: 1 })).toBe('unavailable')
})

test('confirmation temporarily replaces the primary hint and expiry restores it', () => {
  let now = 0
  const controller = new RunInteraction(() => now)
  for (const run of [
    { active: true, owner, foregroundTasks: 1 },
    { active: true, owner, blockingWaits: 1 },
  ]) {
    controller.clear()
    const normal = runStatusPresentation(controller.snapshot(run)).hint
    expect(normal).not.toContain('esc')
    expect(controller.requestInterrupt(run)).toBe('confirm')
    expect(runStatusPresentation(controller.snapshot(run)).hint).toBe(' · esc again to interrupt')
    now += 5000
    expect(runStatusPresentation(controller.snapshot(run)).hint).toBe(normal)
  }
})

test('overlay and queued-message precedence is preserved', () => {
  const input = {
    interaction: resolveRunInteraction({ active: true, owner, blockingWaits: 1 }),
    event: { type: 'escape' as const }, overlay: { kind: 'none' as const },
    editor: createEditorState(), isLoading: true, hasStream: true,
    exitHint: false, logMode: false, hasQueuedPrompt: true,
  }
  expect(decideReplControl(input)).toEqual([{ kind: 'restore-queued' }])
  expect(decideReplControl({ ...input, overlay: { kind: 'help' } })).toEqual([{ kind: 'close-overlay' }])
})
