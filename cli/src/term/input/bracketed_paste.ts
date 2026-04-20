/**
 * Bracketed paste interceptor.
 *
 * Enables bracketed paste mode and strips the markers (\x1b[200~ / \x1b[201~)
 * from stdin before downstream consumers (Ink) see them.
 *
 * When an empty paste is detected (Cmd+V with image in clipboard on macOS),
 * the `onEmptyPaste` callback fires.
 */

import { Transform, type TransformCallback } from 'stream'

const BP_OPEN = '\x1b[200~'
const BP_CLOSE = '\x1b[201~'

export class BracketedPasteTransform extends Transform {
  private inPaste = false
  private pasteContent = ''
  private onEmptyPaste: (() => void) | undefined

  constructor(onEmptyPaste?: () => void) {
    super()
    this.onEmptyPaste = onEmptyPaste
  }

  _transform(chunk: Buffer, _encoding: string, callback: TransformCallback): void {
    let str = chunk.toString('utf8')

    while (true) {
      if (!this.inPaste) {
        const openIdx = str.indexOf(BP_OPEN)
        if (openIdx === -1) {
          // No marker — pass through
          if (str.length > 0) this.push(Buffer.from(str, 'utf8'))
          break
        }
        // Push content before the marker
        if (openIdx > 0) this.push(Buffer.from(str.slice(0, openIdx), 'utf8'))
        str = str.slice(openIdx + BP_OPEN.length)
        this.inPaste = true
        this.pasteContent = ''
      }

      // Inside a paste — look for close marker
      const closeIdx = str.indexOf(BP_CLOSE)
      if (closeIdx === -1) {
        // No close yet — accumulate
        this.pasteContent += str
        break
      }

      this.pasteContent += str.slice(0, closeIdx)
      str = str.slice(closeIdx + BP_CLOSE.length)
      this.inPaste = false

      if (this.pasteContent.length === 0) {
        // Empty paste — likely Cmd+V with image in clipboard
        this.onEmptyPaste?.()
      } else {
        // Non-empty paste — forward content without markers
        this.push(Buffer.from(this.pasteContent, 'utf8'))
        this.pasteContent = ''
      }
    }

    callback()
  }
}

/**
 * Install bracketed paste mode on stdin.
 * Returns a transformed stream that should be passed to Ink as `stdin`,
 * and a cleanup function.
 */
export function installBracketedPaste(
  stdin: NodeJS.ReadStream,
  onEmptyPaste?: () => void,
): { stream: NodeJS.ReadStream; cleanup: () => void } {
  // Enable bracketed paste mode
  process.stdout.write('\x1b[?2004h')

  const transform = new BracketedPasteTransform(onEmptyPaste)

  // Pipe stdin through the transform. We need to cast because Ink expects
  // a ReadStream but Transform is close enough for its purposes.
  stdin.pipe(transform)

  // Copy over properties and methods Ink needs
  const stream = transform as unknown as NodeJS.ReadStream
  Object.defineProperty(stream, 'isTTY', { get: () => stdin.isTTY })
  Object.defineProperty(stream, 'isRaw', { get: () => stdin.isRaw })
  stream.setRawMode = (mode: boolean) => { stdin.setRawMode(mode); return stream }
  stream.ref = () => { stdin.ref(); return stream }
  stream.unref = () => { stdin.unref(); return stream }

  const cleanup = () => {
    stdin.unpipe(transform)
    process.stdout.write('\x1b[?2004l')
  }

  return { stream, cleanup }
}
