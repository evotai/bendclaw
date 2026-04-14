/**
 * ActiveResponse — dynamic zone that re-renders during streaming.
 * Contains the tail message (still updating), streaming text, tool calls, and spinner.
 */

import React from 'react'
import { Box } from 'ink'
import { Message } from './Message.js'
import { StreamingText } from './StreamingText.js'
import { ToolCallDisplay } from './ToolCallDisplay.js'
import { Spinner } from './Spinner.js'
import { RunSummary } from './RunSummary.js'
import { VerboseEventLine } from './VerboseEventLine.js'
import type { UIMessage, UIToolCall, VerboseEvent } from '../state/AppState.js'

interface Props {
  isLoading: boolean
  tailMessage?: UIMessage
  streamText: string
  thinkingText: string
  activeToolCalls: Map<string, UIToolCall>
  outputTokens: number
  verbose: boolean
  verboseEvents: VerboseEvent[]
}

export function ActiveResponse({
  isLoading, tailMessage, streamText, thinkingText,
  activeToolCalls, outputTokens, verbose, verboseEvents,
}: Props) {
  if (!isLoading && !tailMessage) return null

  const hasStream = streamText.length > 0
  const hasThinking = thinkingText.length > 0
  const hasTools = activeToolCalls.size > 0

  return (
    <Box flexDirection="column">
      {/* Tail message — tool status may still be updating */}
      {tailMessage && (
        <React.Fragment>
          {verbose && tailMessage.verboseEvents?.map((evt, i) => (
            <VerboseEventLine key={`tail-evt-${i}`} event={evt} />
          ))}
          <Message message={tailMessage} />
          {verbose && tailMessage.runStats && <RunSummary stats={tailMessage.runStats} />}
        </React.Fragment>
      )}

      {/* Pending verbose events for current turn */}
      {isLoading && verbose && verboseEvents.map((evt, i) => (
        <VerboseEventLine key={`pending-evt-${i}`} event={evt} />
      ))}

      {/* Streaming text */}
      {isLoading && (hasStream || hasThinking) && (
        <StreamingText text={streamText} thinkingText={thinkingText} />
      )}

      {/* Active tool calls */}
      {isLoading && hasTools && (
        <ToolCallDisplay tools={activeToolCalls} />
      )}

      {/* Spinner */}
      {isLoading && !hasStream && !hasThinking && (
        <Spinner
          toolName={hasTools ? [...activeToolCalls.values()][0]?.name : undefined}
          progressText={hasTools ? [...activeToolCalls.values()][0]?.previewCommand : undefined}
          tokenCount={outputTokens}
        />
      )}
    </Box>
  )
}
