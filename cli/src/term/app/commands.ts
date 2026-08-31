import type { Agent, SessionMeta, ConfigInfo } from '../../native/index.js'
import type { AppState } from './state.js'
import type { OutputLine } from '../../render/output.js'
import type { OverlayState } from '../viewmodel/index.js'
import { resolveCommand } from '../../commands/index.js'
import { currentModelSpec, formatModelLabel, modelOptions } from './provider.js'

export interface CommandContext {
  agent: Agent
  appState: AppState
  configInfo?: ConfigInfo
  preloadedSessions: SessionMeta[]
  planning: boolean
}

export interface CommandResult {
  appState: AppState
  planning: boolean
  overlay?: OverlayState
  clearScreen?: boolean
  clearContext?: boolean
  newSession?: boolean
  exit?: boolean
  restart?: boolean
  resumeSession?: SessionMeta
  systemLines: OutputLine[]
}

export function handleSlashCommand(text: string, ctx: CommandContext): CommandResult {
  const resolved = resolveCommand(text)

  if (resolved.kind === 'unknown') {
    return {
      ...baseResult(ctx),
      systemLines: [{ id: 'sys-unk', kind: 'system', text: `  Unknown command: ${text.trim()}` }],
    }
  }

  if (resolved.kind === 'ambiguous') {
    return {
      ...baseResult(ctx),
      systemLines: [
        { id: 'sys-amb', kind: 'system', text: `  Ambiguous command: ${resolved.candidates.join(', ')}` },
        { id: 'sys-amb2', kind: 'system', text: '  Type more characters or /help for commands' },
      ],
    }
  }

  const { name, args } = resolved

  switch (name) {
    case '/help':
      return { ...baseResult(ctx), overlay: { kind: 'help' } }

    case '/new':
      return { ...baseResult(ctx), newSession: true }

    case '/clear':
      return { ...baseResult(ctx), clearContext: true }

    case '/exit':
      return { ...baseResult(ctx), exit: true }

    case '/restart':
      return { ...baseResult(ctx), restart: true }

    case '/plan': {
      const planning = !ctx.planning
      return {
        ...baseResult(ctx),
        planning,
        systemLines: [{ id: 'sys-p', kind: 'system', text: `  plan mode: ${planning ? 'on · write tools disabled' : 'off'}` }],
      }
    }

    case '/model': {
      if (args === 'n') {
        const models = modelOptions(ctx.configInfo, ctx.agent.model)
        if (models.length <= 1) {
          return { ...baseResult(ctx), systemLines: [{ id: 'sys-m', kind: 'system', text: '  Only one model available.' }] }
        }
        const activeSpec = currentModelSpec(ctx.configInfo, ctx.agent.model)
        const idx = models.findIndex(option => option.spec === activeSpec)
        const next = models[(idx + 1) % models.length]!
        ctx.agent.setProvider(next.spec)
        const appState = { ...ctx.appState, model: next.model }
        return {
          ...baseResult(ctx),
          appState,
          systemLines: [{ id: 'sys-model', kind: 'system', text: `  Model → ${formatModelLabel(next.model, next.provider, next.group_label)}` }],
        }
      }
      if (args) {
        const configured = ctx.configInfo?.availableModels.find(option => option.spec === args)
        if (configured) {
          ctx.agent.setProvider(configured.spec)
        } else {
          ctx.agent.model = args
        }
        const model = ctx.agent.model
        const provider = ctx.agent.configInfo().provider
        const appState = { ...ctx.appState, model }
        const selected = configured ?? ctx.configInfo?.availableModels.find(
          option => option.model === model && option.provider === provider)
        return {
          ...baseResult(ctx),
          appState,
          systemLines: [{ id: 'sys-model', kind: 'system', text: `  Model → ${formatModelLabel(model, provider, selected?.group_label)}` }],
        }
      }
      // No arg — return empty result; handleSlashInput will show selector overlay
      return baseResult(ctx)
    }

    case '/resume': {
      // Handled asynchronously in handleSlashInput (needs full session list)
      return baseResult(ctx)
    }

    default:
      // Commands handled asynchronously by handleSlashInput in repl.ts
      return baseResult(ctx)
  }
}

function baseResult(ctx: CommandContext): CommandResult {
  return {
    appState: ctx.appState,
    planning: ctx.planning,
    systemLines: [],
  }
}
