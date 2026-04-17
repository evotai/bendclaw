/**
 * Banner — startup header displayed at the top of the REPL.
 *
 * Three layout modes mirroring Claude Code's LogoV2:
 *
 *   Condensed (< 60 cols):
 *     Logo + info column side by side
 *
 *   Compact (60–99 cols):
 *     Bordered panel, single column: welcome + logo + info
 *
 *   Horizontal (≥ 100 cols):
 *     Bordered panel, two columns: left (welcome + logo + info) | right (feeds)
 *
 * Right-side feed: "Recent sessions".
 */

import React from 'react'
import { Box, Text, useStdout } from 'ink'
import { version, type SessionMeta } from '../native/index.js'

// ── Types ───────────────────────────────────────────────────────────────────

interface FeedLine {
  text: string
  timestamp?: string
}

interface FeedConfig {
  title: string
  lines: FeedLine[]
  footer?: string
  emptyMessage?: string
}

export interface BannerProps {
  model: string
  cwd: string
  sessionId: string | null
  configInfo?: import('../native/index.js').ConfigInfo
  recentSessions?: SessionMeta[]
  releaseNotes?: string[]
}

// ── Logo ────────────────────────────────────────────────────────────────────
// Small block-character icon (9 cols × 3 rows), same size as Claude Code's Clawd.

function Logo() {
  const c = '#3b82f6'
  return (
    <Box flexDirection="column">
      <Text color={c}>{' ▗██████▖ '}</Text>
      <Text color={c}>{'▐████████▌'}</Text>
      <Text color={c}>{' ▀██▀▀██▀ '}</Text>
    </Box>
  )
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function truncatePath(path: string, maxWidth: number): string {
  if (path.length <= maxWidth) return path
  const parts = path.split('/')
  for (let skip = 1; skip < parts.length - 1; skip++) {
    const shortened = parts.slice(0, 1).join('/') + '/…/' + parts.slice(skip + 1).join('/')
    if (shortened.length <= maxWidth) return shortened
  }
  return '…' + path.slice(-(maxWidth - 1))
}

function truncate(text: string, maxWidth: number): string {
  if (text.length <= maxWidth) return text
  return text.slice(0, maxWidth - 1) + '…'
}

function getGitBranch(cwd: string): string | null {
  try {
    const result = Bun.spawnSync(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], {
      cwd,
      stdout: 'pipe',
      stderr: 'pipe',
    })
    if (result.exitCode === 0) {
      return result.stdout.toString().trim()
    }
  } catch { /* not a git repo */ }
  return null
}

function formatRelativeTime(dateStr: string): string {
  const now = Date.now()
  const then = new Date(dateStr).getTime()
  const diffMs = now - then
  const mins = Math.floor(diffMs / 60000)
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins}m ago`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d ago`
  return `${Math.floor(days / 30)}mo ago`
}

type LayoutMode = 'condensed' | 'compact' | 'horizontal'

function getLayoutMode(columns: number): LayoutMode {
  if (columns < 60) return 'condensed'
  if (columns < 100) return 'compact'
  return 'horizontal'
}

const LEFT_PANEL_MAX_WIDTH = 42

// ── Feed ────────────────────────────────────────────────────────────────────

function createRecentSessionsFeed(sessions: SessionMeta[]): FeedConfig {
  const lines: FeedLine[] = sessions.slice(0, 5).map((s) => ({
    text: s.title || '(untitled)',
    timestamp: formatRelativeTime(s.updated_at),
  }))
  return {
    title: 'Recent sessions',
    lines,
    footer: lines.length > 0 ? '/resume for more' : undefined,
    emptyMessage: 'No recent sessions',
  }
}

function createWhatsNewFeed(notes: string[]): FeedConfig {
  const lines: FeedLine[] = notes.slice(0, 4).map((note) => ({ text: note }))
  return {
    title: "What's new",
    lines,
    emptyMessage: 'No release notes',
  }
}

function Feed({ config, width }: { config: FeedConfig; width: number }) {
  return (
    <Box flexDirection="column" paddingY={1}>
      <Text bold dimColor>{config.title}</Text>
      {config.lines.length === 0 && config.emptyMessage && (
        <Text dimColor>  {config.emptyMessage}</Text>
      )}
      {config.lines.map((line, i) => (
        <Box key={i}>
          <Text dimColor>  {truncate(line.text, width - (line.timestamp ? line.timestamp.length + 5 : 4))}</Text>
          {line.timestamp && <Text dimColor>{' '}{line.timestamp}</Text>}
        </Box>
      ))}
      {config.footer && <Text dimColor>  {config.footer}</Text>}
    </Box>
  )
}

function FeedColumn({ feeds, maxWidth }: { feeds: FeedConfig[]; maxWidth: number }) {
  return (
    <Box flexDirection="column">
      {feeds.map((feed, i) => (
        <React.Fragment key={i}>
          <Feed config={feed} width={maxWidth} />
          {i < feeds.length - 1 && (
            <Text dimColor color="#60a5fa">{'─'.repeat(Math.min(maxWidth, 40))}</Text>
          )}
        </React.Fragment>
      ))}
    </Box>
  )
}

// ── Condensed Banner ────────────────────────────────────────────────────────

function CondensedBanner({ model, configInfo, gitBranch, shortCwd, textWidth }: {
  model: string
  configInfo?: import('../native/index.js').ConfigInfo
  gitBranch: string | null
  shortCwd: string
  textWidth: number
}) {
  const provider = configInfo?.provider ?? ''
  const modelLine = provider ? `${model} · ${provider}` : model
  const cwdLine = gitBranch ? `${gitBranch} · ${shortCwd}` : shortCwd

  return (
    <Box flexDirection="row" gap={2} alignItems="center" marginBottom={1}>
      <Logo />
      <Box flexDirection="column">
        <Text>
          <Text bold>evot</Text>{' '}<Text dimColor>v{version()}</Text>
        </Text>
        <Text dimColor>{truncate(modelLine, textWidth)}</Text>
        <Text dimColor>{truncate(cwdLine, textWidth)}</Text>
        {configInfo && !configInfo.hasApiKey && (
          <Text color="yellow">⚠ No API key — edit {configInfo.envPath?.replace(process.env.HOME ?? '', '~') ?? '.env'}</Text>
        )}
      </Box>
    </Box>
  )
}

// ── Compact Banner ──────────────────────────────────────────────────────────

function CompactBanner({ model, configInfo, gitBranch, shortCwd, columns }: {
  model: string
  configInfo?: import('../native/index.js').ConfigInfo
  gitBranch: string | null
  shortCwd: string
  columns: number
}) {
  const provider = configInfo?.provider ?? ''
  const modelLine = provider ? `${model} · ${provider}` : model
  const cwdLine = gitBranch ? `${gitBranch} · ${shortCwd}` : shortCwd

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box
        flexDirection="column"
        borderStyle="round"
        borderColor="#60a5fa"
        paddingX={1}
        paddingY={1}
        alignItems="center"
        width={columns}
      >
        <Text bold>Welcome to evot</Text>
        <Text dimColor>v{version()}</Text>
        <Box marginY={1}><Logo /></Box>
        <Text dimColor>{truncate(modelLine, columns - 4)}</Text>
        <Text dimColor>{truncate(cwdLine, columns - 4)}</Text>
      </Box>
      {configInfo && !configInfo.hasApiKey && (
        <Text color="yellow">  ⚠ No API key configured — edit {configInfo.envPath?.replace(process.env.HOME ?? '', '~') ?? '.env'}</Text>
      )}
      <Text dimColor>  /help commands  ·  Tab complete  ·  ↑↓ history  ·  Ctrl+C×2 exit</Text>
    </Box>
  )
}

// ── Horizontal Banner ───────────────────────────────────────────────────────

function HorizontalBanner({ model, configInfo, gitBranch, shortCwd, columns, recentSessions, releaseNotes }: {
  model: string
  configInfo?: import('../native/index.js').ConfigInfo
  gitBranch: string | null
  shortCwd: string
  columns: number
  recentSessions: SessionMeta[]
  releaseNotes: string[]
}) {
  const provider = configInfo?.provider ?? ''
  const modelLine = provider ? `${model} · ${provider}` : model
  const cwdLine = gitBranch ? `${gitBranch} · ${shortCwd}` : shortCwd

  const leftWidth = LEFT_PANEL_MAX_WIDTH
  const rightWidth = Math.max(columns - leftWidth - 7, 20)

  const feeds: FeedConfig[] = [
    createRecentSessionsFeed(recentSessions),
    createWhatsNewFeed(releaseNotes),
  ]

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box
        flexDirection="column"
        borderStyle="round"
        borderColor="#60a5fa"
      >
        <Box flexDirection="row" paddingX={1} gap={1}>
          {/* Left panel */}
          <Box
            flexDirection="column"
            width={leftWidth}
            justifyContent="space-between"
            alignItems="center"
            minHeight={7}
          >
            <Box marginTop={1} flexDirection="column" alignItems="center">
              <Text bold>Welcome to evot</Text>
              <Text dimColor>v{version()}</Text>
            </Box>
            <Logo />
            <Box flexDirection="column" alignItems="center">
              <Text dimColor>{truncate(modelLine, leftWidth)}</Text>
              <Text dimColor>{truncate(cwdLine, leftWidth)}</Text>
            </Box>
          </Box>

          {/* Divider — borderRight-only Box stretches to full row height */}
          <Box borderStyle="single" borderColor="#60a5fa" borderDimColor
            borderTop={false} borderBottom={false} borderLeft={false} borderRight={true}
          />

          {/* Right panel */}
          <FeedColumn feeds={feeds} maxWidth={rightWidth} />
        </Box>
      </Box>

      {configInfo && !configInfo.hasApiKey && (
        <Text color="yellow">  ⚠ No API key configured — edit {configInfo.envPath?.replace(process.env.HOME ?? '', '~') ?? '.env'}</Text>
      )}
      <Text dimColor>  /help commands  ·  Tab complete  ·  ↑↓ history  ·  Ctrl+C×2 exit</Text>
    </Box>
  )
}

// ── Banner (public) ─────────────────────────────────────────────────────────

export function Banner({ model, cwd, sessionId, configInfo, recentSessions, releaseNotes }: BannerProps) {
  const { stdout } = useStdout()
  const columns = stdout?.columns ?? 80
  const shortCwd = cwd.replace(process.env.HOME ?? '', '~')
  const gitBranch = getGitBranch(cwd)
  const layoutMode = getLayoutMode(columns)

  if (layoutMode === 'condensed') {
    return (
      <CondensedBanner
        model={model}
        configInfo={configInfo}
        gitBranch={gitBranch}
        shortCwd={shortCwd}
        textWidth={Math.max(columns - 15, 20)}
      />
    )
  }

  if (layoutMode === 'compact') {
    return (
      <CompactBanner
        model={model}
        configInfo={configInfo}
        gitBranch={gitBranch}
        shortCwd={shortCwd}
        columns={columns}
      />
    )
  }

  return (
    <HorizontalBanner
      model={model}
      configInfo={configInfo}
      gitBranch={gitBranch}
      shortCwd={shortCwd}
      columns={columns}
      recentSessions={recentSessions ?? []}
      releaseNotes={releaseNotes ?? []}
    />
  )
}
