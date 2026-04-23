import chalk from 'chalk'
import stringWidth from 'string-width'
import { version, type SessionMeta, type ConfigInfo } from '../native/index.js'
import { padRight as formatPadRight, relativeTime } from '../render/format.js'

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
  if (stringWidth(text) <= maxWidth) return text
  let w = 0
  let i = 0
  for (const ch of text) {
    const cw = stringWidth(ch)
    if (w + cw + 1 > maxWidth) break // +1 for '…'
    w += cw
    i += ch.length
  }
  return text.slice(0, i) + '…'
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
  } catch {}
  return null
}


export function renderBanner(
  model: string,
  cwd: string,
  configInfo: ConfigInfo | undefined,
  recentSessions: SessionMeta[],
  columns: number,
  serverState?: { port: number; address: string; channels: string[] } | null,
): string {
  const shortCwd = cwd.replace(process.env.HOME ?? '', '~')
  const gitBranch = getGitBranch(cwd)
  const provider = configInfo?.provider ?? ''
  const modelLine = provider ? `${model} · ${provider}` : model
  const cwdLine = gitBranch ? `${gitBranch} · ${shortCwd}` : shortCwd
  const ver = version()

  const lines: string[] = []

  const logo = [
    ' ▗██████▖ ',
    '▐████████▌',
    ' ▀██▀▀██▀ ',
  ]

  if (columns >= 100) {
    const leftWidth = 42
    const rightWidth = Math.max(columns - leftWidth - 4, 20)
    const border = chalk.hex('#60a5fa')

    const tl = border('╭')
    const tr = border('╮')
    const bl = border('╰')
    const br = border('╯')
    const h = border('─')
    const v = border('│')
    const vdiv = border('│')

    const topBorder = tl + h.repeat(columns - 2) + tr
    const botBorder = bl + h.repeat(columns - 2) + br

    lines.push(topBorder)

    const leftLines: string[] = [
      '',
      center(chalk.bold('Welcome to evot'), leftWidth),
      center(chalk.dim(`v${ver}`), leftWidth),
      '',
      center(chalk.hex('#3b82f6')(logo[0]!), leftWidth),
      center(chalk.hex('#3b82f6')(logo[1]!), leftWidth),
      center(chalk.hex('#3b82f6')(logo[2]!), leftWidth),
      '',
      center(chalk.dim(truncate(modelLine, leftWidth - 2)), leftWidth),
      center(chalk.dim(truncate(cwdLine, leftWidth - 2)), leftWidth),
      '',
    ]

    const rightLines: string[] = ['']
    if (serverState) {
      rightLines.push(chalk.bold.dim('Server'))
      rightLines.push(`  ${chalk.green(serverState.address)}`)
      if (serverState.channels.length > 0) {
        rightLines.push(`  ${chalk.dim('channels: ' + serverState.channels.join(', '))}`)
      }
      rightLines.push('')
    }
    rightLines.push(chalk.bold.dim('Recent sessions'))
    const sessions = recentSessions.slice(0, 5)
    if (sessions.length === 0) {
      rightLines.push(chalk.dim('  No recent sessions'))
    } else {
      for (const s of sessions) {
        const id = s.session_id.slice(0, 8)
        const tag = s.source ? `[${s.source}] ` : ''
        const title = formatPadRight(tag + (s.title || '(untitled)'), 50)
        const turns = formatPadRight(s.turns ? `[${s.turns} turns]` : '', 12)
        const time = relativeTime(s.updated_at)
        rightLines.push(`  ${chalk.dim(id)} ${chalk.dim(title)} ${chalk.dim(turns)} ${chalk.dim(time)}`)
      }
      rightLines.push(chalk.dim('  /resume for more'))
    }
    rightLines.push('')

    const maxHeight = Math.max(leftLines.length, rightLines.length)
    while (leftLines.length < maxHeight) leftLines.push('')
    while (rightLines.length < maxHeight) rightLines.push('')

    for (let i = 0; i < maxHeight; i++) {
      const left = padRight(leftLines[i]!, leftWidth)
      const right = padRight(rightLines[i]!, rightWidth)
      lines.push(`${v} ${left}${vdiv}${right}${v}`)
    }

    lines.push(botBorder)
  } else if (columns >= 60) {
    const border = chalk.hex('#60a5fa')
    const tl = border('╭')
    const tr = border('╮')
    const bl = border('╰')
    const br = border('╯')
    const h = border('─')
    const v = border('│')
    const innerWidth = columns - 4

    lines.push(tl + h.repeat(columns - 2) + tr)
    lines.push(v + ' ' + center(chalk.bold('Welcome to evot'), innerWidth) + ' ' + v)
    lines.push(v + ' ' + center(chalk.dim(`v${ver}`), innerWidth) + ' ' + v)
    lines.push(v + ' '.repeat(columns - 2) + v)
    lines.push(v + ' ' + center(chalk.hex('#3b82f6')(logo[0]!), innerWidth) + ' ' + v)
    lines.push(v + ' ' + center(chalk.hex('#3b82f6')(logo[1]!), innerWidth) + ' ' + v)
    lines.push(v + ' ' + center(chalk.hex('#3b82f6')(logo[2]!), innerWidth) + ' ' + v)
    lines.push(v + ' '.repeat(columns - 2) + v)
    lines.push(v + ' ' + center(chalk.dim(truncate(modelLine, innerWidth - 2)), innerWidth) + ' ' + v)
    lines.push(v + ' ' + center(chalk.dim(truncate(cwdLine, innerWidth - 2)), innerWidth) + ' ' + v)
    lines.push(bl + h.repeat(columns - 2) + br)
  } else {
    lines.push(chalk.hex('#3b82f6')(logo[0]!) + '  ' + chalk.bold('evot') + chalk.dim(` v${ver}`))
    lines.push(chalk.hex('#3b82f6')(logo[1]!) + '  ' + chalk.dim(truncate(modelLine, columns - 14)))
    lines.push(chalk.hex('#3b82f6')(logo[2]!) + '  ' + chalk.dim(truncate(cwdLine, columns - 14)))
  }

  if (configInfo && !configInfo.hasApiKey) {
    const envPath = configInfo.envPath?.replace(process.env.HOME ?? '', '~') ?? '.env'
    lines.push(chalk.yellow(`  ⚠ No API key configured — edit ${envPath}`))
  }
  lines.push(chalk.dim('  /help commands  ·  Tab complete  ·  ↑↓ history  ·  Ctrl+C×2 exit'))
  lines.push('')

  return lines.join('\n')
}

function center(text: string, width: number): string {
  const w = stringWidth(text)
  const pad = Math.max(0, width - w)
  const left = Math.floor(pad / 2)
  return ' '.repeat(left) + text + ' '.repeat(pad - left)
}

function padRight(text: string, width: number): string {
  const w = stringWidth(text)
  const pad = Math.max(0, width - w)
  return text + ' '.repeat(pad)
}
