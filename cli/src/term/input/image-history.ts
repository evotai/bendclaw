import { existsSync } from 'fs'
import { extname } from 'path'
import { formatImageRef, parsePasteRefs } from './paste_refs.js'
import { formatImageSourceText } from './image_store.js'

export interface InputImage {
  id: number
  base64: string
  mediaType: string
  filePath?: string
}

/** Image bindings for input recall, separate from the editable draft's store.
 * Disk history remains escaped plain text, using existing source annotations.
 * No base64 payloads are written to command history.
 */
export class InputImageHistory {
  private readonly images = new Map<number, InputImage>()

  capture(text: string, draft: Map<number, InputImage>): void {
    for (const ref of parsePasteRefs(text)) {
      if (ref.type !== 'image') continue
      const image = draft.get(ref.id)
      if (image) this.images.set(ref.id, { ...image })
    }
  }

  serialize(text: string): string {
    let result = text
    for (const ref of parsePasteRefs(text).reverse()) {
      if (ref.type !== 'image') continue
      const path = this.images.get(ref.id)?.filePath
      if (path) result = result.slice(0, ref.start) + formatImageSourceText(ref.id, path) + result.slice(ref.end)
    }
    return result
  }

  /** Remap every persisted ID, including unavailable images, before new pastes. */
  deserialize(text: string, allocateId: () => number): string {
    const ids = new Map<string, number>()
    return text.replace(/\[Image #(\d+)(?: source: ([^\]\r\n]*))?\]/g, (_match, oldId: string, path?: string) => {
      const key = `${oldId}:${path ?? ''}`
      let id = ids.get(key)
      if (id === undefined) {
        id = allocateId()
        ids.set(key, id)
      }
      if (path) {
        const ext = extname(path).slice(1).toLowerCase()
        const mime = ext === 'jpg' ? 'jpeg' : ext
        if (['png', 'jpeg', 'gif', 'webp'].includes(mime)) {
          this.images.set(id, { id, base64: '', mediaType: `image/${mime}`, filePath: path })
        }
      }
      return formatImageRef(id)
    })
  }

  restore(text: string, draft: Map<number, InputImage>): void {
    for (const ref of parsePasteRefs(text)) {
      if (ref.type !== 'image') continue
      const image = this.images.get(ref.id)
      if (!image) continue
      if (image.filePath && !existsSync(image.filePath)) {
        // A removed cache file must not become a broken path attachment. Live
        // images can fall back to bytes; otherwise keep the unresolved tag.
        if (image.base64) draft.set(ref.id, { ...image, filePath: undefined })
        else draft.delete(ref.id)
      } else {
        draft.set(ref.id, { ...image })
      }
    }
  }
}
