/* auto-generated napi loader — do not edit */
import { createRequire } from 'module'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'

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

  // .node files live in cli/ root
  return require(join(__dirname, '..', '..', filename))
}

const binding = loadBinding()

export const NapiAgent = binding.NapiAgent
export const version = binding.version
export const startServer = binding.startServer
