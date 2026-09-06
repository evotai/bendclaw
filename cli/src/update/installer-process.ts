import stripAnsi from 'strip-ansi'
import { spawn } from 'node:child_process'

export interface InstallerExecution {
  signal?: AbortSignal
  onProgress?: (line: string) => void
}

/** Drain both pipes as the installer runs. Keep a bounded diagnostic tail and
 * render inert text, never curl's carriage-return/terminal control sequences. */
export async function runInstallerScript(script: string, env: Record<string, string>, options: InstallerExecution = {}): Promise<{ success: boolean; output: string }> {
  if (options.signal?.aborted) return { success: false, output: 'Installation cancelled' }
  const proc = spawn('sh', [], { stdio: ['pipe', 'pipe', 'pipe'], env, detached: process.platform !== 'win32' })
  const exited = new Promise<number | null>((resolve, reject) => {
    proc.once('error', reject)
    proc.once('exit', resolve)
  })
  proc.stdin.on('error', () => {})
  proc.stdin.end(script)
  const abort = () => {
    // Stop curl/wget and tar as well as their shell, not just the shell parent.
    try { if (process.platform !== 'win32' && proc.pid) process.kill(-proc.pid, 'SIGKILL'); else proc.kill('SIGKILL') } catch { proc.kill('SIGKILL') }
    // Child pipes can stay open when a descendant inherited the descriptor.
    // Cancellation must not wait on EOF from an installer we already killed.
    proc.stdout.destroy()
    proc.stderr.destroy()
  }
  options.signal?.addEventListener('abort', abort, { once: true })
  if (options.signal?.aborted) abort()
  async function drain(stream: AsyncIterable<Uint8Array>): Promise<string> {
    const decoder = new TextDecoder()
    let tail = ''
    let pending = ''
    const report = (text: string) => {
      const line = stripAnsi(text).replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, '').trim().slice(-1000)
      if (line) { try { options.onProgress?.(line) } catch { /* UI cannot stop an installer. */ } }
    }
    try {
      for await (const value of stream) {
        const text = decoder.decode(value, { stream: true })
        tail = (tail + text).slice(-64 * 1024)
        pending += text
        const lines = pending.split(/[\r\n]/)
        pending = (lines.pop() ?? '').slice(-4096)
        for (const line of lines) report(line)
      }
    } catch (error) {
      if (!options.signal?.aborted) throw error
    }
    const rest = decoder.decode()
    report(pending + rest)
    return (tail + rest).slice(-64 * 1024)
  }
  try {
    const [stdout, stderr, code] = await Promise.all([drain(proc.stdout), drain(proc.stderr), exited])
    if (options.signal?.aborted) return { success: false, output: 'Installation cancelled' }
    return { success: code === 0, output: code === 0 ? stdout : stderr || stdout || `exit code ${code}` }
  } finally {
    options.signal?.removeEventListener('abort', abort)
  }
}
