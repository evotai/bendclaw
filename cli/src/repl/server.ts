import { startServerBackground, type ServerInfo } from '../native/index.js'

export interface ServerState {
  port: number
  address: string
  channels: string[]
  startedAt: number
}

let activePort: number | null = null

export async function tryStartServer(port?: number, envFile?: string): Promise<ServerState | null> {
  const info = await startServerBackground(port, undefined, envFile)
  if (info === null) return null
  activePort = info.port
  return {
    port: info.port,
    address: info.address,
    channels: info.channels,
    startedAt: Date.now(),
  }
}

export function formatUptime(startedAt: number): string {
  const elapsed = Math.floor((Date.now() - startedAt) / 1000)
  if (elapsed < 60) return `${elapsed}s`
  const minutes = Math.floor(elapsed / 60)
  const seconds = elapsed % 60
  if (minutes < 60) return `${minutes}m${seconds.toString().padStart(2, '0')}s`
  const hours = Math.floor(minutes / 60)
  const remainMinutes = minutes % 60
  return `${hours}h${remainMinutes.toString().padStart(2, '0')}m`
}

export function terminalTitle(prefix?: string): string {
  const suffix = activePort ? ` · :${activePort}` : ''
  return prefix ? `${prefix} Evot${suffix}` : `Evot${suffix}`
}

export function setTerminalTitle(prefix?: string): void {
  process.stdout.write(`\x1b]0;${terminalTitle(prefix)}\x07`)
}
