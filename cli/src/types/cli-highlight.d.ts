declare module 'cli-highlight' {
  export function highlight(code: string, options?: { language?: string; ignoreIllegals?: boolean }): string
  export function supportsLanguage(lang: string): boolean
}
