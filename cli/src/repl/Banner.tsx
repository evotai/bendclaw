import React from 'react'
import { Box, Text } from 'ink'
import { version } from '../native/index.js'

export function Banner({ model, cwd, sessionId, configInfo }: {
  model: string
  cwd: string
  sessionId: string | null
  configInfo?: import('../native/index.js').ConfigInfo
}) {
  const shortCwd = cwd.replace(process.env.HOME ?? '', '~')
  const gitBranch = getGitBranch(cwd)
  const sessionLabel = sessionId ? sessionId.slice(0, 8) : '(new)'
  const envPath = configInfo?.envPath?.replace(process.env.HOME ?? '', '~') ?? ''

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box>
        <Text backgroundColor="#60a5fa" color="white" bold>
          {' ◆ evot '}
        </Text>
        <Text dimColor> v{version()}</Text>
      </Box>
      <Text dimColor>  env:      {envPath}</Text>
      {configInfo && !configInfo.hasApiKey && (
        <Text color="yellow">  ⚠ No API key configured — edit {envPath} to set your API keys</Text>
      )}
      {configInfo && <Text dimColor>  provider: {configInfo.provider}</Text>}
      <Text dimColor>  model:    {model}</Text>
      {configInfo?.baseUrl && <Text dimColor>  base_url: {configInfo.baseUrl}</Text>}
      <Text dimColor>  session:  {sessionLabel}</Text>
      {gitBranch && <Text dimColor>  git:      {gitBranch}</Text>}
      <Text dimColor>  cwd:      {shortCwd}</Text>
      <Text dimColor>  /help commands  ·  Tab complete  ·  ↑↓ history  ·  Ctrl+C×2 exit</Text>
    </Box>
  )
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
