/**
 * Read image from system clipboard.
 * macOS: osascript → save PNG to temp → read bytes
 * Linux: xclip / wl-paste
 *
 * Reading is split in two stages because extracting a multi-megabyte image
 * costs hundreds of milliseconds, which is long enough to stall the composer:
 *
 *   probeClipboardImage() — cheap "is there an image, how big is it"
 *   readClipboardImage()  — the expensive byte extraction
 *
 * The caller inserts its placeholder after the probe and loads the bytes in the
 * background, so a large paste shows up immediately instead of after a freeze.
 */

import { execFile } from 'child_process'
import { readFile, unlink } from 'fs/promises'
import { randomBytes } from 'crypto'
import { tmpdir } from 'os'
import { join } from 'path'
import { promisify } from 'util'

const execFileAsync = promisify(execFile)

export const MAX_IMAGE_SIZE_BYTES = 20 * 1024 * 1024 // 20 MB

export interface ClipboardImage {
  base64: string
  mediaType: string
}

export interface ClipboardImageProbe {
  /** Byte size when the platform reports it cheaply, else null. */
  byteLength: number | null
}

function tempPath(): string {
  const suffix = randomBytes(6).toString('hex')
  return join(tmpdir(), `evot_clipboard_${suffix}.png`)
}

/**
 * Detect an image on the clipboard without paying for its bytes.
 * Returns null when the clipboard holds no image.
 */
export async function probeClipboardImage(): Promise<ClipboardImageProbe | null> {
  if (process.platform === 'darwin') return probeMacOS()
  if (process.platform === 'linux') return probeLinux()
  return null
}

/** Extract clipboard image bytes. Callers should probe first. */
export async function readClipboardImage(): Promise<ClipboardImage | null> {
  if (process.platform === 'darwin') return readMacOS()
  if (process.platform === 'linux') return readLinux()
  return null
}

async function probeMacOS(): Promise<ClipboardImageProbe | null> {
  // "clipboard info for «class PNGf»" returns a short description such as
  // "«class PNGf», 4202348" rather than dumping the raw bytes.
  try {
    const { stdout } = await execFileAsync('osascript', [
      '-e', 'clipboard info for «class PNGf»',
    ], { timeout: 3000 })
    if (!stdout || !stdout.includes('PNGf')) return null
    const size = /,\s*(\d+)/.exec(stdout)
    return { byteLength: size ? parseInt(size[1]!, 10) : null }
  } catch {
    return null
  }
}

async function readMacOS(): Promise<ClipboardImage | null> {
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

const LINUX_READERS = [
  ['xclip', ['-selection', 'clipboard', '-t', 'image/png', '-o']],
  ['wl-paste', ['--type', 'image/png']],
] as const

const LINUX_PROBES = [
  ['xclip', ['-selection', 'clipboard', '-t', 'TARGETS', '-o']],
  ['wl-paste', ['--list-types']],
] as const

async function probeLinux(): Promise<ClipboardImageProbe | null> {
  // Listing offered types is cheap; neither tool reports a size, so the byte
  // limit is enforced after extraction.
  for (const [cmd, args] of LINUX_PROBES) {
    try {
      const { stdout } = await execFileAsync(cmd, [...args], { timeout: 3000 })
      if (stdout && stdout.includes('image/png')) return { byteLength: null }
    } catch {
      continue
    }
  }
  return null
}

async function readLinux(): Promise<ClipboardImage | null> {
  for (const [cmd, args] of LINUX_READERS) {
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
