import { resolveRunInteraction } from '../src/term/app/run-interaction.js'
import { expect, test } from 'bun:test'
import stripAnsi from 'strip-ansi'
import { createSpinnerState, setSpinnerPhase, formatSpinnerLine } from '../src/term/spinner.js'
import { createBackgroundOutputState } from '../src/term/app/background-panel.js'
import stringWidth from 'string-width'
import { clipDisplayText } from '../src/render/format.js'

test('background task wait names the agent interrupt target and hides old usage', () => {
  const state = setSpinnerPhase(createSpinnerState(), 'executing', 'task_output')
  const text = stripAnsi(formatSpinnerLine(state, state.phaseStartedAt + 120000, { inputTokens: 100, outputTokens: 40 }, {
    interaction: resolveRunInteraction({ active: true, owner: state, blockingWaits: 1 }),
  }))
  expect(text).toContain('Waiting for task result')
  expect(text).toContain('to release wait')
  expect(text).toContain('esc twice to interrupt')
  expect(text).not.toContain('interrupt agent')
  expect(text).not.toContain('↑')
  expect(text).not.toContain('↓')
  expect(text).not.toContain('to background')
  expect(text).not.toContain('slow')
})
test('command clipping respects grapheme width and background details retain full command', () => {
  const command = '中文🙂'.repeat(80)
  expect(stringWidth(clipDisplayText(command, 12))).toBeLessThanOrEqual(12)
  expect(clipDisplayText('👨‍👩‍👧‍👦 hello', 3)).toBe('👨‍👩‍👧‍👦…')
  const state = createBackgroundOutputState({ task_id: 't', command, cwd: '/tmp', output_path: '/tmp/out', status: 'running', elapsed_ms: 10, exit_code: null, output_file_truncated: false, stopped_by_user: false }, 'output')
  expect(state.items[0]?.preview?.join('\n')).toContain(command)
  expect(state.items[0]?.preview?.join('\n')).toContain('bash')
})
