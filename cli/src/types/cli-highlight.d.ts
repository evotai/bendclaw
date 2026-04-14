declare module 'cli-highlight' {
  export function supportsLanguage(language: string): boolean
  export function highlight(code: string, options?: { language?: string }): string
}
