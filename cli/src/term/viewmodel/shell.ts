import type { OverlayState } from '../app/overlay-state.js'
import { isCommandSelector } from '../app/selector-identity.js'
import type { SelectorState } from '../selector.js'
import type { RenderFrame } from '../render-frame.js'
import { buildCommandSelectorRegion } from './command-selector.js'
import { buildAskRegionLines } from './ask.js'
import { buildSelectorRegionLines } from './selector.js'
import { buildOverlayBlocks } from './overlays.js'
import { buildPromptBlocks, type PromptVMInput } from './prompt.js'
import { buildPromptFooterBlocks } from './prompt-footer.js'
import { blocksToLines, type ViewBlock } from './types.js'

export type CommandPreview = { kind: 'help' } | { kind: 'selector'; state: SelectorState }

export interface ShellSnapshot {
  contentLines: string[]
  preEditorBlocks: ViewBlock[]
  prompt: PromptVMInput
  overlay: OverlayState
  commandFocused: boolean
  preview: CommandPreview | null
}

/** Pure layout composition. The host owns snapshots, scheduling and lifecycle;
 * the renderer owns physical scrollback. Transient surfaces never establish a
 * durable bottom anchor and command preview/focus swaps keep their geometry. */
export function buildShellFrame(input: ShellSnapshot): RenderFrame {
  const { contentLines, prompt, overlay, preview } = input
  const preEditorLines = blocksToLines(input.preEditorBlocks)
  const base = { bottomAnchor: true, bottomAnchorStart: contentLines.length }
  if (overlay.kind === 'selector' && input.commandFocused && isCommandSelector(overlay.state)) {
    const selectorLines = buildCommandSelectorRegion(overlay.state, prompt.columns, prompt.rows, true)
    const promptLines = blocksToLines(buildPromptBlocks(prompt, {
      attachedAbove: true,
      reservedAboveRows: preEditorLines.length + selectorLines.length,
    }))
    return {
      ...base,
      lines: [...contentLines, ...preEditorLines, ...selectorLines, ...promptLines],
      transientRows: selectorLines.length,
      stableViewport: true,
    }
  }
  if (overlay.kind === 'selector' || overlay.kind === 'ask-user') {
    const surfaceLines = overlay.kind === 'selector'
      ? buildSelectorRegionLines(overlay.state, prompt.columns, prompt.rows)
      : buildAskRegionLines(overlay.state, prompt.columns)
    return {
      ...base,
      lines: [...contentLines, ...preEditorLines, ...surfaceLines, ...blocksToLines(buildPromptFooterBlocks(prompt))],
      transientRows: surfaceLines.length,
    }
  }
  const modalLines = blocksToLines(buildOverlayBlocks(overlay, prompt.columns))
  const previewLines = preview
    ? preview.kind === 'selector'
      ? buildCommandSelectorRegion(preview.state, prompt.columns, prompt.rows, false)
      : blocksToLines(buildOverlayBlocks({ kind: 'help' }, prompt.columns))
    : []
  const promptLines = blocksToLines(buildPromptBlocks(prompt, {
    attachedAbove: preEditorLines.length > 0 || previewLines.length > 0,
    reservedAboveRows: preEditorLines.length + previewLines.length,
  }))
  return {
    ...base,
    lines: [...contentLines, ...preEditorLines, ...previewLines, ...promptLines],
    transientRows: previewLines.length,
    ...(preview?.kind === 'selector' ? { stableViewport: true } : {}),
    ...(modalLines.length > 0 ? { overlay: { lines: modalLines } } : {}),
  }
}
