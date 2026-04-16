import { describe, test, expect, beforeEach, afterEach } from 'bun:test'
import { mkdirSync, writeFileSync, existsSync, rmSync } from 'fs'
import { join } from 'path'
import { tmpdir } from 'os'
import { _testing } from '../src/commands/log-share.js'

const { toDownloadUrl, encrypt, decrypt, validateAndImport, listFilesRecursive, generatePassword } = _testing

// ---------------------------------------------------------------------------
// toDownloadUrl
// ---------------------------------------------------------------------------

describe('toDownloadUrl', () => {
  test('inserts /dl/ for tmpfiles.org URLs', () => {
    expect(toDownloadUrl('https://tmpfiles.org/12345/evot-log.bin'))
      .toBe('https://tmpfiles.org/dl/12345/evot-log.bin')
  })

  test('handles http variant', () => {
    expect(toDownloadUrl('http://tmpfiles.org/99/file.bin'))
      .toBe('http://tmpfiles.org/dl/99/file.bin')
  })

  test('returns other URLs unchanged', () => {
    expect(toDownloadUrl('https://example.com/file.bin'))
      .toBe('https://example.com/file.bin')
  })
})

// ---------------------------------------------------------------------------
// generatePassword
// ---------------------------------------------------------------------------

describe('generatePassword', () => {
  test('generates 8-char password', () => {
    const pw = generatePassword()
    expect(pw.length).toBe(8)
  })

  test('only contains expected characters', () => {
    const allowed = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789'
    for (let i = 0; i < 20; i++) {
      const pw = generatePassword()
      for (const ch of pw) {
        expect(allowed.includes(ch)).toBe(true)
      }
    }
  })
})

// ---------------------------------------------------------------------------
// encrypt / decrypt round-trip
// ---------------------------------------------------------------------------

describe('encrypt / decrypt', () => {
  test('round-trip preserves data', () => {
    const original = Buffer.from('hello world — session log data')
    const { payload, password } = encrypt(original)
    const result = decrypt(payload, password)
    expect(result.equals(original)).toBe(true)
  })

  test('wrong password fails', () => {
    const original = Buffer.from('secret data')
    const { payload } = encrypt(original)
    expect(() => decrypt(payload, 'WrOnGpWd')).toThrow()
  })

  test('bad magic fails', () => {
    const bad = Buffer.from('BADMAGIC' + '0'.repeat(44))
    expect(() => decrypt(bad, 'whatever')).toThrow('Invalid file format')
  })

  test('too small payload fails', () => {
    const tiny = Buffer.from('short')
    expect(() => decrypt(tiny, 'whatever')).toThrow('too small')
  })
})

// ---------------------------------------------------------------------------
// validateAndImport
// ---------------------------------------------------------------------------

describe('validateAndImport', () => {
  const SID = 'abcdef01-2345-6789-abcd-ef0123456789'
  let tmpDir: string

  beforeEach(() => {
    tmpDir = join(tmpdir(), `evot-test-validate-${Date.now()}`)
    mkdirSync(tmpDir, { recursive: true })
  })

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true })
  })

  test('imports valid session files', () => {
    // Create valid structure
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    mkdirSync(join(tmpDir, 'logs'), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    writeFileSync(join(tmpDir, 'sessions', SID, 'transcript.jsonl'), '')
    writeFileSync(join(tmpDir, 'logs', `${SID}.log`), 'log data')

    const targetDir = join(tmpdir(), `evot-test-target-${Date.now()}`)
    mkdirSync(targetDir, { recursive: true })

    const result = validateAndImport(tmpDir, targetDir)
    expect(result).toBe(SID)

    // Verify files were moved to target
    expect(existsSync(join(targetDir, 'sessions', SID, 'session.json'))).toBe(true)
    expect(existsSync(join(targetDir, 'sessions', SID, 'transcript.jsonl'))).toBe(true)
    expect(existsSync(join(targetDir, 'logs', `${SID}.log`))).toBe(true)

    rmSync(targetDir, { recursive: true, force: true })
  })

  test('rejects path traversal', () => {
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    // Create a file that would be listed with ..
    mkdirSync(join(tmpDir, '..hack'), { recursive: true })
    writeFileSync(join(tmpDir, '..hack', 'evil.txt'), 'bad')

    // listFilesRecursive won't produce ".." paths from normal extraction,
    // but validateAndImport rejects unexpected files
    expect(() => validateAndImport(tmpDir)).toThrow('Rejected unsafe path')
  })

  test('rejects unexpected files', () => {
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    writeFileSync(join(tmpDir, 'sessions', SID, 'extra.txt'), 'bad')

    expect(() => validateAndImport(tmpDir)).toThrow('Unexpected file')
  })

  test('rejects multiple session ids', () => {
    const SID2 = '11111111-2222-3333-4444-555555555555'
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    mkdirSync(join(tmpDir, 'sessions', SID2), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    writeFileSync(join(tmpDir, 'sessions', SID2, 'session.json'), '{}')

    expect(() => validateAndImport(tmpDir)).toThrow('multiple sessions')
  })

  test('rejects empty archive', () => {
    expect(() => validateAndImport(tmpDir)).toThrow('Could not determine session id')
  })
})

// ---------------------------------------------------------------------------
// listFilesRecursive
// ---------------------------------------------------------------------------

describe('listFilesRecursive', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = join(tmpdir(), `evot-test-list-${Date.now()}`)
    mkdirSync(tmpDir, { recursive: true })
  })

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true })
  })

  test('lists nested files with relative paths', () => {
    mkdirSync(join(tmpDir, 'a', 'b'), { recursive: true })
    writeFileSync(join(tmpDir, 'a', 'b', 'c.txt'), '')
    writeFileSync(join(tmpDir, 'top.txt'), '')

    const files = listFilesRecursive(tmpDir)
    expect(files.sort()).toEqual(['a/b/c.txt', 'top.txt'])
  })

  test('returns empty for empty dir', () => {
    expect(listFilesRecursive(tmpDir)).toEqual([])
  })
})
