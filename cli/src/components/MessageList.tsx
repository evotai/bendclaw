import React from 'react'
import { Box } from 'ink'
import type { UIMessage, VerboseEvent, RunStats } from '../state/AppState.js'
import { Message } from './Message.js'
import { RunSummary } from './RunSummary.js'

interface MessageListProps {
  messages: UIMessage[]
  verbose: boolean
  renderVerboseEvent: (event: VerboseEvent, key: string) => React.ReactNode
}

function MessageListInner({ messages, verbose, renderVerboseEvent }: MessageListProps) {
  return (
    <Box flexDirection="column">
      {messages.map((msg) => (
        <React.Fragment key={msg.id}>
          {verbose && msg.verboseEvents?.map((evt, i) => (
            <React.Fragment key={`${msg.id}-evt-${i}`}>
              {renderVerboseEvent(evt, `${msg.id}-evt-${i}`)}
            </React.Fragment>
          ))}
          <Message message={msg} />
          {verbose && msg.runStats && <RunSummary stats={msg.runStats as RunStats} />}
        </React.Fragment>
      ))}
    </Box>
  )
}

export const MessageList = React.memo(MessageListInner)
