/**
 * Read image from system clipboard.
 * macOS: osascript → save PNG to temp → read as base64
 * Linux: xclip / wl-paste
 */

import { execFile } from 'child_process'
import { readFile, unlink } from 'fs/promises'
import { randomBytes } from 'crypto'
import { tmpdir } from 'os'
import { join } from 'path'
import { promisify } from 'util'

const execFileAsync = promisify(execFile)

const MAX_IMAGE_SIZE_BYTES = 20 * 1024 * 1024 // 20 MB

export interface ClipboardImage {
  base64: string
  mediaType: string
}

function tempPath(): string {
  const suffix = randomBytes(6).toString('hex')
  return join(tmpdir(), `evot_clipboard_${suffix}.png`)
}

export async function getImageFromClipboard(): Promise<ClipboardImage | null> {
  if (process.platform === 'darwin') return getImageMacOS()
  if (process.platform === 'linux') return getImageLinux()
  return null
}

async function getImageMacOS(): Promise<ClipboardImage | null> {
  // Check if clipboard contains image data without reading the full payload.
  // "clipboard info for «class PNGf»" returns a small text description
  // (e.g. "«class PNGf», 123456") instead of dumping the raw bytes.
  try {
    const { stdout } = await execFileAsync('osascript', [
      '-e', 'clipboard info for «class PNGf»',
    ], { timeout: 3000 })
    // If the clipboard has no PNG data, osascript errors or returns empty
    if (!stdout || !stdout.includes('PNGf')) return null
  } catch {
    return null
  }

  const path = tempPath()
  try {
    await execFileAsync('osascript', [
      '-e', 'set png_data to (the clipboard as «class PNGf»)',
      '-e', `set fp to open for access POSIX file "${path}" with write permission`,
      '-e', 'write png_data to fp',
      '-e', 'close access fp',
    ], { timeout: 30000 })

    const buffer = await readFile(path)
    unlink(path).catch(() => {})

    if (buffer.length === 0) return null
    if (buffer.length > MAX_IMAGE_SIZE_BYTES) return null

    return {
      base64: buffer.toString('base64'),
      mediaType: detectMediaType(buffer),
    }
  } catch {
    unlink(path).catch(() => {})
    return null
  }
}

async function getImageLinux(): Promise<ClipboardImage | null> {
  // Try xclip first, then wl-paste
  for (const [cmd, args] of [
    ['xclip', ['-selection', 'clipboard', '-t', 'image/png', '-o']],
    ['wl-paste', ['--type', 'image/png']],
  ] as const) {
    try {
      const result = await execFileAsync(cmd, [...args], {
        encoding: 'buffer',
        timeout: 3000,
        maxBuffer: MAX_IMAGE_SIZE_BYTES,
      } as any)
      const buffer = result.stdout as unknown as Buffer
      if (buffer && buffer.length > 0 && buffer.length <= MAX_IMAGE_SIZE_BYTES) {
        return {
          base64: buffer.toString('base64'),
          mediaType: detectMediaType(buffer),
        }
      }
    } catch {
      continue
    }
  }
  return null
}

/** Detect image format from magic bytes. */
function detectMediaType(buffer: Buffer): string {
  if (buffer.length >= 2) {
    if (buffer[0] === 0x89 && buffer[1] === 0x50) return 'image/png'
    if (buffer[0] === 0xFF && buffer[1] === 0xD8) return 'image/jpeg'
    if (buffer[0] === 0x47 && buffer[1] === 0x49) return 'image/gif'
    if (buffer[0] === 0x52 && buffer[1] === 0x49) return 'image/webp'
  }
  return 'image/png'
}
