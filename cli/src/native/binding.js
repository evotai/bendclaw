/* auto-generated napi loader — do not edit */
import { createRequire } from 'module'
import { join, dirname, resolve } from 'path'
import { fileURLToPath } from 'url'
import { existsSync } from 'fs'
import { homedir } from 'os'

const __dirname = dirname(fileURLToPath(import.meta.url))
const require = createRequire(import.meta.url)

function loadBinding() {
  const platform = process.platform
  const arch = process.arch

  const triples = {
    'darwin-arm64': 'evot-napi.darwin-arm64.node',
    'darwin-x64': 'evot-napi.darwin-x64.node',
    'linux-x64': 'evot-napi.linux-x64-gnu.node',
    'linux-arm64': 'evot-napi.linux-arm64-gnu.node',
  }

  const key = `${platform}-${arch}`
  const filename = triples[key]
  if (!filename) {
    throw new Error(`Unsupported platform: ${key}`)
  }

  // Search order:
  // 1. cli/ root (dev mode, relative to this file) — so local builds win
  // 2. EVOT_HOME/lib/ (installed)
  const evotHome = process.env.EVOT_HOME || join(homedir(), '.evotai')
  const candidates = [
    join(__dirname, '..', '..', filename),
    join(evotHome, 'lib', filename),
  ]

  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return require(resolve(candidate))
    }
  }

  throw new Error(
    `Cannot find ${filename} in any of:\n${candidates.map(c => `  - ${c}`).join('\n')}`
  )
}

const binding = loadBinding()

export const NapiAgent = binding.NapiAgent
export const version = binding.version
export const startServer = binding.startServer
export const startServerBackground = binding.startServerBackground
