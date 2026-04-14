/**
 * AskUser — structured question UI for tool permission prompts.
 * Ported from Rust ask_user.rs state machine.
 *
 * NOTE: Not yet wired end-to-end — the NAPI binding doesn't surface
 * ask_user events. This component is ready for when that's added.
 */

import React, { useState } from 'react'
import { Text, Box, useInput } from 'ink'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface AskUserQuestion {
  header: string
  question: string
  options: { label: string; description?: string }[]
}

export interface AskUserRequest {
  questions: AskUserQuestion[]
}

export interface AskUserAnswer {
  questionIndex: number
  selectedOption: number | null  // null = custom text
  customText?: string
}

interface AskUserProps {
  request: AskUserRequest
  onSubmit: (answers: AskUserAnswer[]) => void
  onCancel: () => void
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function AskUser({ request, onSubmit, onCancel }: AskUserProps) {
  const { questions } = request
  const [activeQuestion, setActiveQuestion] = useState(0)
  const [selected, setSelected] = useState(0)
  const [answers, setAnswers] = useState<(AskUserAnswer | null)[]>(
    () => questions.map(() => null)
  )
  const [typing, setTyping] = useState(false)
  const [draft, setDraft] = useState('')

  const q = questions[activeQuestion]!
  const optionCount = q.options.length + 1 // +1 for "Other"

  useInput((ch, key) => {
    if (typing) {
      // Typing mode — custom text input
      if (key.escape) {
        setTyping(false)
        setDraft('')
        return
      }
      if (key.return) {
        if (draft.trim().length > 0) {
          confirmAnswer({ questionIndex: activeQuestion, selectedOption: null, customText: draft.trim() })
        } else {
          setTyping(false)
        }
        return
      }
      if (key.backspace || key.delete) {
        setDraft(prev => prev.slice(0, -1))
        return
      }
      if (ch && !key.ctrl && !key.meta) {
        setDraft(prev => prev + ch)
      }
      return
    }

    // Selection mode
    if (key.upArrow) {
      setSelected(prev => (prev - 1 + optionCount) % optionCount)
      return
    }
    if (key.downArrow) {
      setSelected(prev => (prev + 1) % optionCount)
      return
    }
    if (key.leftArrow && questions.length > 1) {
      setActiveQuestion(prev => Math.max(0, prev - 1))
      setSelected(0)
      return
    }
    if (key.rightArrow && questions.length > 1) {
      setActiveQuestion(prev => Math.min(questions.length - 1, prev + 1))
      setSelected(0)
      return
    }
    if (key.return) {
      if (selected === q.options.length) {
        // "Other" selected — enter typing mode
        setTyping(true)
        setDraft('')
      } else {
        confirmAnswer({ questionIndex: activeQuestion, selectedOption: selected })
      }
      return
    }
    // Digit shortcuts (1-9)
    const digit = parseInt(ch, 10)
    if (digit >= 1 && digit <= q.options.length) {
      confirmAnswer({ questionIndex: activeQuestion, selectedOption: digit - 1 })
      return
    }
    if (key.escape) {
      onCancel()
      return
    }
    if (key.ctrl && ch === 'c') {
      onCancel()
      return
    }
  })

  function confirmAnswer(answer: AskUserAnswer) {
    const newAnswers = [...answers]
    newAnswers[answer.questionIndex] = answer
    setAnswers(newAnswers)
    setTyping(false)
    setDraft('')

    // Find next unanswered question
    const nextUnanswered = newAnswers.findIndex(a => a === null)
    if (nextUnanswered === -1) {
      // All answered — submit
      onSubmit(newAnswers as AskUserAnswer[])
    } else {
      setActiveQuestion(nextUnanswered)
      setSelected(0)
    }
  }

  return (
    <Box flexDirection="column" marginBottom={1}>
      {/* Tab bar for multi-question */}
      {questions.length > 1 && (
        <Box marginBottom={1}>
          {questions.map((qq, i) => {
            const answered = answers[i] !== null
            const active = i === activeQuestion
            const check = answered ? '✓' : ' '
            return (
              <Box key={i} marginRight={2}>
                <Text color={active ? 'cyan' : undefined} bold={active}>
                  [{check}] {qq.header}
                </Text>
              </Box>
            )
          })}
        </Box>
      )}

      {/* Question text */}
      <Box marginBottom={1}>
        <Text color="cyan" bold>{q.header}: </Text>
        <Text>{q.question}</Text>
      </Box>

      {/* Options */}
      {q.options.map((opt, i) => {
        const isSelected = !typing && i === selected
        return (
          <Box key={i}>
            <Text color={isSelected ? 'cyan' : undefined} bold={isSelected}>
              {isSelected ? '❯ ' : '  '}
              {i + 1}. {opt.label}
            </Text>
            {opt.description && <Text dimColor> — {opt.description}</Text>}
          </Box>
        )
      })}

      {/* "Other" option */}
      <Box>
        <Text
          color={!typing && selected === q.options.length ? 'cyan' : undefined}
          bold={!typing && selected === q.options.length}
        >
          {!typing && selected === q.options.length ? '❯ ' : '  '}
          Other
        </Text>
      </Box>

      {/* Typing input */}
      {typing && (
        <Box marginTop={1}>
          <Text color="cyan" bold>{'> '}</Text>
          <Text>{draft}</Text>
          <Text inverse>{' '}</Text>
        </Box>
      )}

      {/* Footer hints */}
      <Box marginTop={1}>
        <Text dimColor italic>
          {typing
            ? 'Enter to confirm · Esc to cancel'
            : `↑↓ navigate · Enter select · 1-${q.options.length} shortcut · Esc cancel`}
        </Text>
      </Box>
    </Box>
  )
}
