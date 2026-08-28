/**
 * Render the real ad slot to stdout, so the dithered band can be eyeballed.
 *
 *   cd cli && bun scripts/preview-ad-slot-live.ts
 */
import chalk from 'chalk'
import { createAdSlotState, triggerAdSlot, tickAdSlot, buildAdSlotBlocks } from '../src/term/viewmodel/ad-slot.js'
import { styledLineToAnsi } from '../src/term/viewmodel/types.js'

chalk.level = 3

const T0 = 1_000_000
const columns = process.stdout.columns ?? 120

const state = createAdSlotState([
  {
    id: 'live',
    kind: 'notice',
    title: 'GPT-5.6 Luna is free through Aug 31 🎉',
    body: 'Run `/login` - Sponsored by **Databend Cloud** · Agent-Ready Data Warehouse → [databend.com](https://databend.com)',
  },
])
triggerAdSlot(state, T0)

// Far enough in that typing has finished, before the erase phase starts.
const now = T0 + 15_000
const blocks = buildAdSlotBlocks(state, tickAdSlot(state, now), columns, now)

console.log()
for (const block of blocks) {
  for (const l of block.lines) console.log(styledLineToAnsi(l))
}
console.log()
