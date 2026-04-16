/**
 * log-share — pack, encrypt, upload / download, decrypt, import session logs.
 *
 * Encryption format:
 *   EVOTLOG1 (8 B magic) | salt (16 B) | IV (12 B) | authTag (16 B) | AES-256-GCM ciphertext
 *   Key derived from short password via PBKDF2 (100k iterations, SHA-256).
 *
 * Upload target: tmpfiles.org (free, no auth required).
 */

import { execFileSync } from 'child_process'
import { createCipheriv, createDecipheriv, randomBytes, pbkdf2Sync } from 'crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync, statSync, renameSync, rmSync } from 'fs'
import https from 'https'
import http from 'http'
import { homedir, tmpdir } from 'os'
import { join } from 'path'

const MAGIC = 'EVOTLOG1'
const EVOTAI_DIR = join(homedir(), '.evotai')
const PBKDF2_ITERATIONS = 100_000
const PASSWORD_LENGTH = 8
const PASSWORD_CHARS = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789' // no ambiguous chars

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export interface PutResult {
  url: string
}

export interface GetResult {
  sessionId: string
}

/**
 * Pack, encrypt, and upload a session's logs.
 */
export async function logPut(sessionId: string): Promise<PutResult> {
  const files = collectFiles(sessionId)
  if (files.length === 0) {
    throw new Error(`No files found for session ${sessionId}`)
  }

  // tar czf into a temp file
  const tarPath = join(tmpdir(), `evot-export-${sessionId.slice(0, 8)}.tar.gz`)
  try {
    execFileSync('tar', ['czf', tarPath, ...files], { cwd: EVOTAI_DIR })
  } catch (err: any) {
    throw new Error(`tar failed: ${err?.message ?? err}`)
  }

  // Encrypt
  const plaintext = readFileSync(tarPath)
  const { payload, password } = encrypt(plaintext)

  // Clean up tar
  rmSync(tarPath, { force: true })

  // Upload
  const rawUrl = await upload(payload)
  const url = `${rawUrl}#${password}`

  return { url }
}

/**
 * Download, decrypt, and import a shared session.
 */
export async function logGet(urlWithKey: string): Promise<GetResult> {
  const hashIdx = urlWithKey.lastIndexOf('#')
  if (hashIdx < 0) {
    throw new Error('URL must contain a #password fragment')
  }
  const baseUrl = urlWithKey.slice(0, hashIdx)
  const password = urlWithKey.slice(hashIdx + 1)
  if (!password) {
    throw new Error('Password is empty')
  }

  // Convert tmpfiles.org URL to download URL
  const downloadUrl = toDownloadUrl(baseUrl)

  // Download
  const payload = await download(downloadUrl)

  // Decrypt
  let decrypted: Buffer
  try {
    decrypted = decrypt(payload, password)
  } catch {
    throw new Error('Decryption failed — wrong password or corrupted file')
  }

  // Extract to temp dir
  const tmpDir = join(tmpdir(), `evot-import-${Date.now()}`)
  mkdirSync(tmpDir, { recursive: true })
  const tarPath = join(tmpDir, 'export.tar.gz')
  writeFileSync(tarPath, decrypted)

  try {
    execFileSync('tar', ['xzf', tarPath], { cwd: tmpDir })
  } catch (err: any) {
    rmSync(tmpDir, { recursive: true, force: true })
    throw new Error(`tar extract failed: ${err?.message ?? err}`)
  }
  rmSync(tarPath, { force: true })

  // Validate and import
  const sessionId = validateAndImport(tmpDir)

  // Clean up
  rmSync(tmpDir, { recursive: true, force: true })

  return { sessionId }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

function collectFiles(sessionId: string): string[] {
  const files: string[] = []
  const sessionDir = join('sessions', sessionId)
  const candidates = [
    join(sessionDir, 'session.json'),
    join(sessionDir, 'transcript.jsonl'),
    join('logs', `${sessionId}.log`),
    join('logs', `${sessionId}.screen.log`),
  ]
  for (const f of candidates) {
    if (existsSync(join(EVOTAI_DIR, f))) {
      files.push(f)
    }
  }
  return files
}

/** Validate extracted files and move them into the target dir (default ~/.evotai) */
function validateAndImport(tmpDir: string, targetRoot?: string): string {
  const destRoot = targetRoot ?? EVOTAI_DIR
  const allowedPattern = /^(sessions\/[0-9a-f-]+\/(session\.json|transcript\.jsonl)|logs\/[0-9a-f-]+\.(log|screen\.log))$/

  // Enumerate all files
  const allFiles = listFilesRecursive(tmpDir)
  let sessionId: string | null = null

  for (const rel of allFiles) {
    // Security: reject path traversal, absolute paths, symlinks
    if (rel.includes('..') || rel.startsWith('/')) {
      throw new Error(`Rejected unsafe path: ${rel}`)
    }
    const fullPath = join(tmpDir, rel)
    const stat = statSync(fullPath)
    if (stat.isSymbolicLink()) {
      throw new Error(`Rejected symbolic link: ${rel}`)
    }
    if (!allowedPattern.test(rel)) {
      throw new Error(`Unexpected file in archive: ${rel}`)
    }

    // Extract session id
    const match = rel.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/)
    if (match) {
      if (sessionId && sessionId !== match[0]) {
        throw new Error('Archive contains files from multiple sessions')
      }
      sessionId = match[0]
    }
  }

  if (!sessionId) {
    throw new Error('Could not determine session id from archive')
  }

  // Move files into place
  const targetSessionDir = join(destRoot, 'sessions', sessionId)
  const targetLogsDir = join(destRoot, 'logs')
  mkdirSync(targetSessionDir, { recursive: true })
  mkdirSync(targetLogsDir, { recursive: true })

  for (const rel of allFiles) {
    const src = join(tmpDir, rel)
    const dst = join(destRoot, rel)
    mkdirSync(join(dst, '..'), { recursive: true })
    renameSync(src, dst)
  }

  return sessionId
}

function listFilesRecursive(dir: string, prefix = ''): string[] {
  const results: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const rel = prefix ? `${prefix}/${entry.name}` : entry.name
    if (entry.isDirectory()) {
      results.push(...listFilesRecursive(join(dir, entry.name), rel))
    } else {
      results.push(rel)
    }
  }
  return results
}

/** Convert tmpfiles.org URL to its download variant (insert /dl/) */
function toDownloadUrl(url: string): string {
  // https://tmpfiles.org/12345/file.bin → https://tmpfiles.org/dl/12345/file.bin
  const m = url.match(/^(https?:\/\/tmpfiles\.org)\/([\d]+\/.+)$/)
  if (m) {
    return `${m[1]}/dl/${m[2]}`
  }
  return url
}

/** Upload a buffer to tmpfiles.org, return the raw URL. */
function upload(data: Buffer): Promise<string> {
  return new Promise((resolve, reject) => {
    const boundary = `----evot${Date.now()}`
    const header = `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="evot-log.bin"\r\nContent-Type: application/octet-stream\r\n\r\n`
    const footer = `\r\n--${boundary}--\r\n`
    const body = Buffer.concat([Buffer.from(header), data, Buffer.from(footer)])

    const req = https.request(
      {
        hostname: 'tmpfiles.org',
        path: '/api/v1/upload',
        method: 'POST',
        headers: {
          'Content-Type': `multipart/form-data; boundary=${boundary}`,
          'Content-Length': body.length,
        },
      },
      (res) => {
        let raw = ''
        res.on('data', (chunk: Buffer) => (raw += chunk))
        res.on('end', () => {
          if (res.statusCode !== 200) {
            reject(new Error(`Upload failed (HTTP ${res.statusCode}): ${raw.slice(0, 200)}`))
            return
          }
          try {
            const json = JSON.parse(raw)
            if (json.status === 'success' && json.data?.url) {
              // tmpfiles.org returns http://, normalize to https://
              const url = (json.data.url as string).replace(/^http:\/\//, 'https://')
              resolve(url)
            } else {
              reject(new Error(`Unexpected response: ${raw.slice(0, 200)}`))
            }
          } catch {
            reject(new Error(`Failed to parse response: ${raw.slice(0, 200)}`))
          }
        })
      },
    )
    req.on('error', reject)
    req.write(body)
    req.end()
  })
}

// ---------------------------------------------------------------------------
// Test helpers — exported for unit tests only
// ---------------------------------------------------------------------------

export const _testing = { toDownloadUrl, encrypt, decrypt, validateAndImport, listFilesRecursive, generatePassword }

function generatePassword(): string {
  const bytes = randomBytes(PASSWORD_LENGTH)
  let result = ''
  for (let i = 0; i < PASSWORD_LENGTH; i++) {
    result += PASSWORD_CHARS[bytes[i]! % PASSWORD_CHARS.length]
  }
  return result
}

function deriveKey(password: string, salt: Buffer): Buffer {
  return pbkdf2Sync(password, salt, PBKDF2_ITERATIONS, 32, 'sha256')
}

/** Encrypt: EVOTLOG1 (8B) | salt (16B) | IV (12B) | authTag (16B) | ciphertext */
function encrypt(plaintext: Buffer): { payload: Buffer; password: string } {
  const password = generatePassword()
  const salt = randomBytes(16)
  const key = deriveKey(password, salt)
  const iv = randomBytes(12)
  const cipher = createCipheriv('aes-256-gcm', key, iv)
  const encrypted = Buffer.concat([cipher.update(plaintext), cipher.final()])
  const authTag = cipher.getAuthTag()
  const magicBuf = Buffer.from(MAGIC)
  const payload = Buffer.concat([magicBuf, salt, iv, authTag, encrypted])
  return { payload, password }
}

/** Decrypt: parse EVOTLOG1 (8B) | salt (16B) | IV (12B) | authTag (16B) | ciphertext */
function decrypt(payload: Buffer, password: string): Buffer {
  const minSize = 8 + 16 + 12 + 16 // magic + salt + iv + authTag
  if (payload.length < minSize) {
    throw new Error('File too small to be a valid export')
  }
  const magic = payload.subarray(0, 8).toString()
  if (magic !== MAGIC) {
    throw new Error('Invalid file format — not an evot log export')
  }
  const salt = payload.subarray(8, 24)
  const iv = payload.subarray(24, 36)
  const authTag = payload.subarray(36, 52)
  const ciphertext = payload.subarray(52)
  const key = deriveKey(password, salt)
  const decipher = createDecipheriv('aes-256-gcm', key, iv)
  decipher.setAuthTag(authTag)
  return Buffer.concat([decipher.update(ciphertext), decipher.final()])
}

/** Download a URL, following redirects. */
function download(url: string, redirects = 5): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    if (redirects <= 0) {
      reject(new Error('Too many redirects'))
      return
    }
    const proto = url.startsWith('https') ? https : http
    proto.get(url, (res) => {
      if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        resolve(download(res.headers.location, redirects - 1))
        return
      }
      if (res.statusCode !== 200) {
        let body = ''
        res.on('data', (chunk: Buffer) => (body += chunk))
        res.on('end', () => reject(new Error(`Download failed (HTTP ${res.statusCode}): ${body.slice(0, 200)}`)))
        return
      }
      const chunks: Buffer[] = []
      res.on('data', (chunk: Buffer) => chunks.push(chunk))
      res.on('end', () => resolve(Buffer.concat(chunks)))
    }).on('error', reject)
  })
}
