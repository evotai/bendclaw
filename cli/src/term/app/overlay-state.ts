import type { AskState } from '../ask.js'
import type { SelectorState } from '../selector.js'

/** Application-owned transient surface state. Views consume this union but
 * commands and input routing never need to import the renderer to change it. */
export type OverlayState =
  | { kind: 'none' }
  | { kind: 'help' }
  | { kind: 'selector'; state: SelectorState }
  | { kind: 'ask-user'; state: AskState }
