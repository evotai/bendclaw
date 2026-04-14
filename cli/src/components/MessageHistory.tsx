/**
 * MessageHistory — renders committed messages using ink's <Static>.
 * Items are appended once and never re-rendered, eliminating flicker.
 * The banner is rendered as the first static item.
 */

import React from 'react'
import { Static } from 'ink'
import { Message } from './Message.js'
import { RunSummary } from './RunSummary.js'
import { VerboseEventLine } from './VerboseEventLine.js'
import type { UIMessage } from '../state/AppState.js'

type StaticItem =
  | { kind: 'banner'; id: string; node: React.ReactNode }
  | { kind: 'message'; id: string; msg: UIMessage }

interface Props {
  banner: React.ReactNode
  messages: UIMessage[]
  verbose: boolean
}

export function MessageHistory({ banner, messages, verbose }: Props) {
  const items: StaticItem[] = [
    { kind: 'banner', id: '__banner__', node: banner },
    ...messages.map((msg) => ({ kind: 'message' as const, id: msg.id, msg })),
  ]

  return (
    <Static items={items}>
      {(item) => {
        if (item.kind === 'banner') {
          return <React.Fragment key={item.id}>{item.node}</React.Fragment>
        }
        const msg = item.msg
        return (
          <React.Fragment key={item.id}>
            {verbose && msg.verboseEvents?.map((evt, i) => (
              <VerboseEventLine key={`${item.id}-evt-${i}`} event={evt} />
            ))}
            <Message message={msg} />
            {verbose && msg.runStats && (
              <RunSummary stats={msg.runStats} />
            )}
          </React.Fragment>
        )
      }}
    </Static>
  )
}
