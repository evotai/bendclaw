import { describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'fs'
import { join } from 'path'
import { tmpdir } from 'os'

import { parseRequires } from '../src/commands/skill/frontmatter.js'
import { commitFromRoot } from '../src/commands/skill/fetch.js'
import { readSourceRecord } from '../src/commands/skill/install.js'
import { skillInstall, skillRemove, skillUpdate } from '../src/commands/skill/manage.js'
import { isValidSkillName, scanSkillDir } from '../src/commands/skill/scan.js'
import { resolveSource } from '../src/commands/skill/source.js'
import { enumerateUnits, supersededDirs } from '../src/commands/skill/units.js'
import type { Checkout } from '../src/commands/skill/fetch.js'
import type { OperationView, SkillOutcome, UnitResult } from '../src/commands/skill/render.js'

/** The operation report, or a failure when the call refused instead. */
function view(outcome: SkillOutcome): OperationView {
  if (!('view' in outcome)) throw new Error(`expected an operation, got notice: ${outcome.notice}`)
  return outcome.view
}

/** The one-line refusal, or a failure when the call actually ran. */
function notice(outcome: SkillOutcome): string {
  if ('view' in outcome) throw new Error('expected a notice, got an operation report')
  return outcome.notice
}

function unit(outcome: SkillOutcome, name: string): UnitResult {
  const found = view(outcome).units.find((entry) => entry.name === name)
  if (!found) throw new Error(`no unit named ${name}`)
  return found
}

function noteTexts(result: UnitResult): string[] {
  return result.notes.map((note) => note.text)
}

let counter = 0

function workspace(): string {
  const dir = join(tmpdir(), `evot-skill-${process.pid}-${Date.now()}-${counter++}`)
  mkdirSync(dir, { recursive: true })
  return dir
}

function vars(root: string): string {
  return join(root, 'variables.json')
}

function writeSkill(dir: string, name: string, frontmatter = ''): void {
  mkdirSync(dir, { recursive: true })
  writeFileSync(
    join(dir, 'SKILL.md'),
    `---\nname: ${name}\ndescription: ${name} skill\n${frontmatter}---\n\n# ${name}\n`,
  )
}

function fakeRepo(root: string, commit: string): Checkout {
  writeSkill(join(root, 'skills', 'databend-cloud'), 'databend-cloud')
  mkdirSync(join(root, 'skills', 'databend-cloud', 'scripts'), { recursive: true })
  writeFileSync(join(root, 'skills', 'databend-cloud', 'scripts', 'query.py'), 'print(1)\n')
  writeSkill(join(root, 'skills', 'lark', 'lark-im'), 'lark-im')
  writeSkill(join(root, 'skills', 'lark', 'lark-shared'), 'lark-shared')
  writeFileSync(join(root, 'skills', 'lark', 'README.md'), 'group\n')
  return { dir: root, commit }
}

function stubFetch(commit: string): { fetch: () => Promise<Checkout>; calls: number } {
  const state = { calls: 0 }
  return {
    get calls() {
      return state.calls
    },
    fetch: async () => {
      state.calls += 1
      return fakeRepo(workspace(), commit)
    },
  }
}

describe('resolveSource', () => {
  test('no argument selects the whole official repo', () => {
    expect(resolveSource(undefined, {})).toEqual({
      repo: 'evotai/evot-skills',
      ref: 'main',
      official: true,
    })
  })

  test('bare name selects one official unit', () => {
    expect(resolveSource('lark', {})).toEqual({
      repo: 'evotai/evot-skills',
      ref: 'main',
      path: 'skills/lark',
      official: true,
    })
  })

  test('EVOT_SKILLS_REPO overrides the official repo', () => {
    expect(resolveSource(undefined, { EVOT_SKILLS_REPO: 'acme/mirror' }).repo).toBe('acme/mirror')
  })

  test('owner/repo and refs resolve to third-party sources', () => {
    expect(resolveSource('acme/skills', {})).toEqual({
      repo: 'acme/skills',
      ref: 'main',
      official: false,
    })
    expect(resolveSource('acme/skills@v2', {})).toMatchObject({ repo: 'acme/skills', ref: 'v2' })
  })

  test('github urls carry ref and subpath', () => {
    expect(resolveSource('https://github.com/acme/skills/tree/dev/pack/one', {})).toEqual({
      repo: 'acme/skills',
      ref: 'dev',
      path: 'pack/one',
      official: false,
    })
  })

  test('rejects invalid names and sources', () => {
    expect(() => resolveSource('bad name', {})).toThrow()
    expect(() => resolveSource('http://example.com/x', {})).toThrow()
  })
})

describe('enumerateUnits', () => {
  test('official repo yields one unit per skills/ child', () => {
    const { dir } = fakeRepo(workspace(), 'abc1234')
    const units = enumerateUnits(dir, resolveSource(undefined, {}))
    expect(units.map((unit) => unit.name)).toEqual(['databend-cloud', 'lark'])
    expect(units[0]!.skills.map((skill) => skill.name)).toEqual(['databend-cloud'])
    expect(units[1]!.skills.map((skill) => skill.name)).toEqual(['lark-im', 'lark-shared'])
    expect(units[1]!.skills.every((skill) => skill.group === 'lark')).toBe(true)
  })

  test('named official unit resolves alone', () => {
    const { dir } = fakeRepo(workspace(), 'abc1234')
    const units = enumerateUnits(dir, resolveSource('lark', {}))
    expect(units).toHaveLength(1)
    expect(units[0]!.path).toBe('skills/lark')
  })

  test('missing official unit fails loudly', () => {
    const { dir } = fakeRepo(workspace(), 'abc1234')
    expect(() => enumerateUnits(dir, resolveSource('nope', {}))).toThrow('No SKILL.md found')
  })

  test('third-party repo root SKILL.md becomes one unit named after the repo', () => {
    const dir = workspace()
    writeSkill(dir, 'solo')
    const units = enumerateUnits(dir, resolveSource('acme/solo-skill', {}))
    expect(units.map((unit) => unit.name)).toEqual(['solo-skill'])
  })

  test('third-party repo falls back to top-level dirs', () => {
    const dir = workspace()
    writeSkill(join(dir, 'alpha'), 'alpha')
    writeSkill(join(dir, 'beta'), 'beta')
    writeFileSync(join(dir, 'README.md'), 'x\n')
    expect(enumerateUnits(dir, resolveSource('acme/pack', {})).map((u) => u.name)).toEqual([
      'alpha',
      'beta',
    ])
  })

  test('third-party repo prefers a skills/ directory when present', () => {
    const dir = workspace()
    writeSkill(join(dir, 'skills', 'alpha'), 'alpha')
    writeSkill(join(dir, 'docs'), 'docs')
    expect(enumerateUnits(dir, resolveSource('acme/pack', {})).map((u) => u.name)).toEqual(['alpha'])
  })

  test('repo without any SKILL.md fails', () => {
    const dir = workspace()
    mkdirSync(join(dir, 'docs'), { recursive: true })
    expect(() => enumerateUnits(dir, resolveSource('acme/pack', {}))).toThrow('No SKILL.md found')
  })
})

describe('supersededDirs', () => {
  test('flat copies of grouped skills are superseded', () => {
    const root = workspace()
    writeSkill(join(root, 'lark-im'), 'lark-im')
    writeSkill(join(root, 'zero-tech-debt'), 'zero-tech-debt')
    const { dir } = fakeRepo(workspace(), 'abc1234')
    const unit = enumerateUnits(dir, resolveSource('lark', {}))[0]!

    expect(supersededDirs(root, unit).map((entry) => entry.name)).toEqual(['lark-im'])
  })

  test('reinstalling in place supersedes nothing', () => {
    const root = workspace()
    writeSkill(join(root, 'lark', 'lark-im'), 'lark-im')
    const { dir } = fakeRepo(workspace(), 'abc1234')
    const unit = enumerateUnits(dir, resolveSource('lark', {}))[0]!

    expect(supersededDirs(root, unit)).toEqual([])
  })
})

describe('skillInstall', () => {
  test('installs every official unit and records the source', async () => {
    const root = workspace()
    const stub = stubFetch('abc1234')
    const out = await skillInstall(undefined, { root, variablesFile: vars(root), fetch: stub.fetch, env: {} })

    expect(view(out).title).toBe('Installed')
    expect(view(out).source).toBe('evotai/evot-skills@abc1234')
    expect(view(out).total).toBe(3)
    expect(view(out).units.map((entry) => [entry.name, entry.skills, entry.outcome])).toEqual([
      ['databend-cloud', 1, 'new'],
      ['lark', 2, 'new'],
    ])
    expect(existsSync(join(root, 'databend-cloud', 'scripts', 'query.py'))).toBe(true)
    expect(existsSync(join(root, 'lark', 'lark-shared', 'SKILL.md'))).toBe(true)
    expect(readSourceRecord(join(root, 'lark'))).toMatchObject({
      repo: 'evotai/evot-skills',
      ref: 'main',
      path: 'skills/lark',
      commit: 'abc1234',
    })
    expect(scanSkillDir(root).map((entry) => entry.name)).toEqual([
      'databend-cloud',
      'lark-im',
      'lark-shared',
    ])
  })

  test('replaces flat skills that the group now provides', async () => {
    const root = workspace()
    writeSkill(join(root, 'lark-im'), 'lark-im')
    writeSkill(join(root, 'diagram-design'), 'diagram-design')
    const stub = stubFetch('abc1234')

    const out = await skillInstall('lark', { root, variablesFile: vars(root), fetch: stub.fetch, env: {} })
    expect(noteTexts(unit(out, 'lark'))).toContain('replaced standalone lark-im')
    expect(unit(out, 'lark').notes[0]!.kind).toBe('info')
    expect(existsSync(join(root, 'lark-im'))).toBe(false)
    expect(existsSync(join(root, 'diagram-design'))).toBe(true)
    expect(existsSync(join(root, 'lark', 'lark-im', 'SKILL.md'))).toBe(true)
  })

  test('reinstall is idempotent and refreshes content', async () => {
    const root = workspace()
    const stub = stubFetch('abc1234')
    await skillInstall('databend-cloud', { root, variablesFile: vars(root), fetch: stub.fetch, env: {} })
    writeFileSync(join(root, 'databend-cloud', 'SKILL.md'), 'tampered')

    await skillInstall('databend-cloud', { root, variablesFile: vars(root), fetch: stub.fetch, env: {} })
    expect(readFileSync(join(root, 'databend-cloud', 'SKILL.md'), 'utf8')).toContain(
      'name: databend-cloud',
    )
  })

  test('reports missing requirements after install', async () => {
    const root = workspace()
    const source = workspace()
    writeSkill(
      join(source, 'skills', 'databend-cloud'),
      'databend-cloud',
      'metadata:\n  evot:\n    requires:\n      env: [BENDCLOUD_DSN]\n    envHints:\n      BENDCLOUD_DSN: bendcloud://org:token@api.databend.com/default\n',
    )

    const out = await skillInstall('databend-cloud', {
      root,
      variablesFile: vars(root),
      fetch: async () => ({ dir: source, commit: 'abc1234' }),
      env: {},
    })
    expect(noteTexts(unit(out, 'databend-cloud'))).toEqual([
      'needs /env set BENDCLOUD_DSN=bendcloud://org:token@api.databend.com/default',
    ])
    expect(unit(out, 'databend-cloud').notes[0]!.kind).toBe('warn')
  })

  test('satisfied requirements produce no note', async () => {
    const root = workspace()
    const source = workspace()
    writeSkill(
      join(source, 'skills', 'databend-cloud'),
      'databend-cloud',
      'metadata:\n  evot:\n    requires:\n      env: [BENDCLOUD_DSN]\n',
    )

    const out = await skillInstall('databend-cloud', {
      root,
      variablesFile: vars(root),
      fetch: async () => ({ dir: source, commit: 'abc1234' }),
      env: { BENDCLOUD_DSN: 'bendcloud://org:token@api.databend.com/default' },
    })
    expect(unit(out, 'databend-cloud').notes).toEqual([])
  })
})

describe('skillUpdate', () => {
  test('refreshes tracked units and reports new commits', async () => {
    const root = workspace()
    await skillInstall(undefined, { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })

    const next = stubFetch('def5678')
    const out = await skillUpdate(undefined, { root, variablesFile: vars(root), fetch: next.fetch, env: {} })

    expect(next.calls).toBe(1)
    expect(view(out).title).toBe('Updated')
    expect(unit(out, 'lark').outcome).toBe('updated')
    expect(unit(out, 'lark').detail).toBe('abc1234 → def5678')
    expect(readSourceRecord(join(root, 'lark'))?.commit).toBe('def5678')
  })

  test('same commit reports unchanged', async () => {
    const root = workspace()
    await skillInstall('lark', { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })

    const out = await skillUpdate(undefined, { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })
    expect(unit(out, 'lark').outcome).toBe('unchanged')
    expect(unit(out, 'lark').detail).toBe('abc1234')
  })

  test('local units are skipped, not touched', async () => {
    const root = workspace()
    writeSkill(join(root, 'zero-tech-debt'), 'zero-tech-debt')
    await skillInstall('lark', { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })

    const out = await skillUpdate(undefined, { root, variablesFile: vars(root), fetch: stubFetch('def5678').fetch, env: {} })
    expect(unit(out, 'zero-tech-debt')).toMatchObject({ outcome: 'skipped', detail: 'local' })
    expect(existsSync(join(root, 'zero-tech-debt', 'SKILL.md'))).toBe(true)
  })

  test('named update rejects unknown and local units', async () => {
    const root = workspace()
    writeSkill(join(root, 'zero-tech-debt'), 'zero-tech-debt')

    expect(notice(await skillUpdate('missing', { root, variablesFile: vars(root), env: {} }))).toContain('not installed')
    expect(notice(await skillUpdate('zero-tech-debt', { root, variablesFile: vars(root), env: {} }))).toContain('no install source')
    expect(notice(await skillUpdate('bad name', { root, variablesFile: vars(root), env: {} }))).toContain('invalid skill name')
  })

  test('nothing tracked yields a clear message', async () => {
    const root = workspace()
    expect(notice(await skillUpdate(undefined, { root, variablesFile: vars(root), env: {} }))).toBe('no updatable skills installed')
  })

  test('third-party repo-root units stay updatable', async () => {
    const root = workspace()
    const rootSkillRepo = (commit: string) => async (): Promise<Checkout> => {
      const dir = workspace()
      writeSkill(dir, 'solo')
      return { dir, commit }
    }
    await skillInstall('acme/solo', {
      root,
      variablesFile: vars(root),
      fetch: rootSkillRepo('aaa1111'),
      env: {},
    })

    expect(readSourceRecord(join(root, 'solo'))).toMatchObject({ repo: 'acme/solo', path: '' })

    const out = await skillUpdate(undefined, {
      root,
      variablesFile: vars(root),
      env: {},
      fetch: rootSkillRepo('bbb2222'),
    })
    expect(view(out).units.map((entry) => entry.outcome)).toEqual(['updated'])
    expect(readSourceRecord(join(root, 'solo'))?.commit).toBe('bbb2222')
  })

  test('fetch failure is reported per unit without throwing', async () => {
    const root = workspace()
    await skillInstall('lark', { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })

    const out = await skillUpdate(undefined, {
      root,
      variablesFile: vars(root),
      env: {},
      fetch: async () => {
        throw new Error('network down')
      },
    })
    expect(unit(out, 'lark')).toMatchObject({ outcome: 'failed', detail: 'network down' })
    expect(existsSync(join(root, 'lark', 'lark-im', 'SKILL.md'))).toBe(true)
  })
})

describe('skillRemove', () => {
  test('removes a whole group by unit name', async () => {
    const root = workspace()
    await skillInstall('lark', { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })

    expect(skillRemove('lark', root)).toEqual({
      notice: 'removed skill group: lark (2 skills)',
      removed: true,
    })
    expect(existsSync(join(root, 'lark'))).toBe(false)
  })

  test('removes one nested skill by its own name', async () => {
    const root = workspace()
    await skillInstall('lark', { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })

    const result = skillRemove('lark-im', root)
    expect(result.removed).toBe(true)
    expect(result.notice).toContain('from group lark')
    expect(existsSync(join(root, 'lark', 'lark-im'))).toBe(false)
    expect(existsSync(join(root, 'lark', 'lark-shared'))).toBe(true)
  })

  test('reports unknown and invalid names', () => {
    const root = workspace()
    expect(skillRemove('nope', root)).toEqual({ notice: 'skill not found: nope', removed: false })
    expect(skillRemove('bad name', root)).toEqual({
      notice: 'invalid skill name: bad name',
      removed: false,
    })
  })

  test('refuses traversal names instead of deleting the parent tree', () => {
    const root = join(workspace(), 'skills')
    mkdirSync(root, { recursive: true })
    writeSkill(join(root, 'keeper'), 'keeper')

    for (const name of ['..', '.', '.hidden', '../..']) {
      expect(skillRemove(name, root)).toMatchObject({ removed: false })
      expect(skillRemove(name, root).notice).toContain('invalid skill name')
    }
    expect(existsSync(root)).toBe(true)
    expect(existsSync(join(root, 'keeper', 'SKILL.md'))).toBe(true)
  })
})

describe('isValidSkillName', () => {
  test('accepts plain skill names', () => {
    for (const name of ['lark', 'lark-im', 'databend-cloud', '_private', 'a1.2']) {
      expect(isValidSkillName(name)).toBe(true)
    }
  })

  test('rejects dot-leading and separator-bearing names', () => {
    for (const name of ['.', '..', '.hidden', '-lead', 'a/b', 'a b', '']) {
      expect(isValidSkillName(name)).toBe(false)
    }
  })
})

describe('commitFromRoot', () => {
  test('reads the sha from the tarball root directory', () => {
    expect(commitFromRoot('evotai-evot-skills-b5d29ee/README.md\n')).toBe('b5d29ee')
  })

  test('handles hyphenated repo names', () => {
    expect(commitFromRoot('acme-my-cool-skills-0123abcdef/skills/\n')).toBe('0123abcdef')
  })

  test('unexpected listings degrade to unknown', () => {
    expect(commitFromRoot('weird\n')).toBe('unknown')
    expect(commitFromRoot('')).toBe('unknown')
  })
})

describe('readSourceRecord', () => {
  test('unsupported versions and malformed json are not managed', async () => {
    const root = workspace()
    await skillInstall('lark', { root, variablesFile: vars(root), fetch: stubFetch('abc1234').fetch, env: {} })
    const file = join(root, 'lark', '.evot-source.json')
    const record = JSON.parse(readFileSync(file, 'utf8')) as Record<string, unknown>

    expect(readSourceRecord(join(root, 'lark'))).toMatchObject({ version: 1 })

    writeFileSync(file, JSON.stringify({ ...record, version: 2 }))
    expect(readSourceRecord(join(root, 'lark'))).toBeNull()

    writeFileSync(file, '{ not json')
    expect(readSourceRecord(join(root, 'lark'))).toBeNull()
  })
})

describe('parseRequires', () => {
  test('reads the evot namespace', () => {
    const parsed = parseRequires(
      '---\nname: x\nmetadata:\n  evot:\n    requires:\n      env: [A, B]\n      bins: [python3]\n---\n',
    )
    expect(parsed.env).toEqual(['A', 'B'])
    expect(parsed.bins).toEqual(['python3'])
  })

  test('reads the bare metadata.requires shape used by lark-cli', () => {
    const parsed = parseRequires(
      '---\nname: lark-im\nmetadata:\n  requires:\n    bins: ["lark-cli"]\n  cliHelp: "lark-cli im --help"\n---\n',
    )
    expect(parsed.bins).toEqual(['lark-cli'])
    expect(parsed.env).toEqual([])
  })

  test('reads block sequences and envVars descriptions', () => {
    const parsed = parseRequires(
      '---\nname: x\nmetadata:\n  evot:\n    requires:\n      env:\n        - A\n        - B\n    envVars:\n      - name: A\n        description: token for A\n      - name: C\n        required: false\n---\n',
    )
    expect(parsed.env).toEqual(['A', 'B'])
    expect(parsed.envHints.A).toBe('token for A')
    expect(parsed.env).not.toContain('C')
  })

  test('missing or malformed frontmatter yields empty requirements', () => {
    expect(parseRequires('no frontmatter')).toEqual({ env: [], bins: [], envHints: {} })
    expect(parseRequires('---\nname: x\n')).toEqual({ env: [], bins: [], envHints: {} })
  })
})
