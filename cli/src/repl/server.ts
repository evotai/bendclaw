import { startServerBackground } from '../native/index.js'

export interface ServerState {
  port: number
  startedAt: number
}

export async function tryStartServer(port?: number): Promise<ServerState | null> {
  const result = await startServerBackground(port)
  if (result === null) return null
  return { port: result, startedAt: Date.now() }
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

export function setTerminalTitle(serverState: ServerState | null): void {
  const title = serverState ? `Evot · :${serverState.port}` : 'Evot'
  process.stdout.write(`\x1b]0;${title}\x07`)
}
