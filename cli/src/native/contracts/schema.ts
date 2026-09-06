/** Small JSON contract readers. Diagnostics contain schema paths only, never
 * values from tool output, credentials or model responses. Unknown keys survive. */
export interface Schema<T> {
  read(value: unknown, path: string): T
}
export type Infer<S> = S extends Schema<infer T> ? T : never

function invalid(path: string): never {
  throw new Error(`Invalid query event at ${path}`)
}

export const text: Schema<string> = { read(value, path) {
  if (typeof value !== 'string') invalid(path)
  return value
} }
export const boolean: Schema<boolean> = { read(value, path) {
  if (typeof value !== 'boolean') invalid(path)
  return value
} }
// Rust unsigned counters may exceed JS safe precision in existing wire JSON.
// Reject negative/fractional/non-finite values, not otherwise readable u64s.
export const uint: Schema<number> = { read(value, path) {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) invalid(path)
  return value
} }
export const json: Schema<unknown> = { read(value, path) {
  if (value === undefined) invalid(path)
  return value
} }
export function optional<T>(schema: Schema<T>): Schema<T | undefined> {
  return { read: (value, path) => value === undefined ? undefined : schema.read(value, path) }
}
export function nullable<T>(schema: Schema<T>): Schema<T | null> {
  return { read: (value, path) => value === null ? null : schema.read(value, path) }
}
export function oneOf<const T extends readonly string[]>(...values: T): Schema<T[number]> {
  return { read(value, path) {
    if (typeof value !== 'string' || !values.includes(value)) invalid(path)
    return value as T[number]
  } }
}
export function array<T>(schema: Schema<T>): Schema<T[]> {
  return { read(value, path) {
    if (!Array.isArray(value)) invalid(path)
    value.forEach((entry, index) => schema.read(entry, `${path}[${index}]`))
    return value as T[]
  } }
}
export function object<const S extends Record<string, Schema<unknown>>>(fields: S): Schema<{ [K in keyof S]: Infer<S[K]> }> {
  return { read(value, path) {
    if (value === null || typeof value !== 'object' || Array.isArray(value)) invalid(path)
    const record = value as Record<string, unknown>
    for (const [key, schema] of Object.entries(fields)) schema.read(record[key], `${path}.${key}`)
    return value as { [K in keyof S]: Infer<S[K]> }
  } }
}
export function tagged<const S extends Record<string, Schema<unknown>>>(key: string, variants: S): Schema<Infer<S[keyof S]>> {
  return { read(value, path) {
    if (value === null || typeof value !== 'object' || Array.isArray(value)) invalid(path)
    const tag = (value as Record<string, unknown>)[key]
    if (typeof tag !== 'string' || !Object.hasOwn(variants, tag)) invalid(`${path}.${key}`)
    return variants[tag]!.read(value, path) as Infer<S[keyof S]>
  } }
}
export const toolDetail: Schema<[string, number]> = { read(value, path) {
  if (!Array.isArray(value) || value.length !== 2) invalid(path)
  text.read(value[0], `${path}[0]`)
  uint.read(value[1], `${path}[1]`)
  return value as [string, number]
} }
