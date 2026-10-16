import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute } from 'node:path';
import { projectEvents } from '../model/session';
import type { EventEnvelopeV1 } from './protocol';
import { blake3 } from '@noble/hashes/blake3.js';
import type { Methods, Method, UsageAggregate, UsageHistory } from './protocol';

export type ExtensionFixtures = Partial<{ [M in Method]: Methods[M]['result'] }>;
export class FixtureExtensions {
  readonly values: ExtensionFixtures;
  private blobs = new Map<string, { mime: string; dataBase64: string }>();
  private scratchSerial = 0;
  constructor(
    values: ExtensionFixtures = {},
    private eventsForRun: (runDir: string) => EventEnvelopeV1[] = () => {
      throw new Error('Fixture session not found');
    },
  ) {
    this.values = structuredClone(values);
  }
  dispatch(method: Method, p: Record<string, any>): unknown {
    const configured = this.values[method];
    if (
      configured !== undefined &&
      !['skills.list', 'memory.list', 'toggles.get', 'sessions.exportMarkdown'].includes(method)
    )
      return structuredClone(configured);
    const skills = (this.values['skills.list'] ??= {
      skills: [
        {
          id: 'fixture-code-review',
          name: 'code-review',
          description: 'Review code changes for correctness and maintainability.',
          scope: 'user',
          project: null,
          enabled: true,
          body: '# Code review\n\nRead the change and explain concrete correctness risks before suggesting improvements.',
          allowedTools: ['read', 'grep'],
          primaryDirectory: '/fixture/home/skills/code-review',
          primaryFile: '/fixture/home/skills/code-review/SKILL.md',
          installs: [
            {
              directory: '/fixture/home/skills/code-review',
              file: '/fixture/home/skills/code-review/SKILL.md',
              root: '/fixture/home/skills',
              scope: 'user',
              source: 'Harness',
              enabled: true,
            },
          ],
          supportingFiles: 1,
          sizeBytes: 1640,
          updatedAt: 1792108800000,
        },
      ],
      errors: [],
    });
    const memory = (this.values['memory.list'] ??= {
      store_path: '/fixture/home/projects/project/memory.json',
      entries: [
        {
          key: 'project.build',
          value: 'Use bun for the desktop frontend and cargo for the backend.',
          updated_at_unix_ms: 1792108800000,
          scope: 'workspace',
        },
      ],
    });
    switch (method) {
      case 'sessions.attachment': {
        const blob = this.blobs.get(p.ref);
        if (!blob) throw new Error('Fixture attachment not found');
        return { dataBase64: blob.dataBase64 };
      }
      case 'worktrees.list':
        return { repository_root: p.cwd, managed_count: 0, worktrees: [] };
      case 'worktrees.remove': {
        const list = this.values['worktrees.list'];
        const entry = list?.worktrees.find(
          (entry) => entry.harness_managed && entry.slug === p.slug,
        );
        if (!entry || !list) throw new Error('Managed worktree not found');
        list.worktrees = list.worktrees.filter((saved) => saved !== entry);
        list.managed_count = list.worktrees.filter((entry) => entry.harness_managed).length;
        return {
          removed: true,
          slug: entry.slug,
          path: entry.path,
          branch: entry.branch,
          delete_branch: !p.keepBranch,
        };
      }
      case 'diagnostics.rebuildSessionIndex':
        return {
          entry_count: 0,
          journals_scanned: 0,
          journals_opened: 0,
          index_path: '/fixture/sessions/session-index.json',
          skipped_changing_journals: 0,
        };
      case 'diagnostics.crashScan':
        return {
          scan_root: '/fixture/sessions',
          summary: {
            scanned: 0,
            previous_crash: 0,
            clean: 0,
            stale_writer_lock: 0,
            recovery_marker: 0,
          },
          reports: [],
        };
      case 'diagnostics.refreshModelCatalog':
        return { message: 'Fixture model catalog refreshed; no network request was made.' };
      case 'providers.protocols':
        return [
          {
            protocol: 'open_ai_compatible',
            support: 'supported',
            apis: ['responses', 'chat_completions'],
          },
          { protocol: 'anthropic_messages', support: 'supported', apis: ['messages'] },
        ];
      case 'config.validate':
        return { valid: true, message: 'Fixture configuration is valid.' };
      case 'config.explain':
        return {
          schema_version: 'harness-config-explain-v1',
          path: p.path,
          redacted: true,
          found: false,
          effective: null,
          source_path: null,
          primary_path: null,
          layers: [],
          source_note: 'No fixture layer defines this path.',
        };
      case 'blobs.put': {
        const bytes = Buffer.from(p.dataBase64, 'base64');
        if (bytes.toString('base64') !== p.dataBase64) throw new Error('Invalid base64');
        const digest = `attachment:blake3:${Buffer.from(blake3(bytes)).toString('hex')}`;
        this.blobs.set(digest, { mime: p.mime, dataBase64: p.dataBase64 });
        return { digest };
      }
      case 'blobs.get':
        return this.blobs.get(p.digest) ?? null;
      case 'skills.list':
        return skills;
      case 'skills.read': {
        const skill = skills.skills.find((s) => s.id === p.id);
        if (!skill) throw new Error('Skill not found');
        return { id: skill.id, body: skill.body };
      }
      case 'skills.setEnabled': {
        const skill = skills.skills.find((s) => s.id === p.id);
        if (!skill) throw new Error('Skill not found');
        const changed = skill.installs.filter((i) => i.enabled !== p.enabled).length;
        skill.enabled = p.enabled;
        for (const i of skill.installs) {
          i.enabled = p.enabled;
          i.file = `${i.directory}/SKILL.md${p.enabled ? '' : '.disabled'}`;
        }
        skill.primaryFile = skill.installs[0]?.file ?? skill.primaryFile;
        return { id: p.id, enabled: p.enabled, changed };
      }
      case 'skills.trash': {
        const index = skills.skills.findIndex((s) => s.id === p.id);
        if (index < 0) throw new Error('Skill not found');
        const [skill] = skills.skills.splice(index, 1);
        return { id: p.id, trashed: skill!.installs.length };
      }
      case 'usage.history':
        return usage(p.from, p.to, p.cwd ?? '/fixture/project');
      case 'usage.subscription':
        return {
          provider: p.provider,
          status: 'unconfigured',
          plan: null,
          windows: [],
          message: 'No subscription OAuth credential is configured in this fixture.',
        };
      case 'memory.list':
        return memory;
      case 'memory.search':
        return {
          ...memory,
          entries: memory.entries.filter((e) =>
            `${e.key} ${e.value}`.toLowerCase().includes(p.query.toLowerCase()),
          ),
        };
      case 'memory.get': {
        const entry = memory.entries.find((e) => e.key === p.key);
        if (!entry) throw new Error('Memory not found');
        return entry;
      }
      case 'memory.delete': {
        const index = memory.entries.findIndex((e) => e.key === p.key);
        if (index < 0) return { deleted: false };
        memory.entries.splice(index, 1);
        return { deleted: true };
      }
      case 'plans.list':
        return {
          plans: [
            {
              path: `${p.cwd}/.harness/plans/desktop.md`,
              slug: 'desktop',
              exists: true,
              is_active: true,
              byte_len: 98,
            },
          ],
        };
      case 'plans.read':
        return {
          path: p.path,
          content:
            '# Desktop implementation\n\n- Connect the session coordinator.\n- Review the changed files.\n- Run scoped checks.\n',
        };
      case 'history.prompts':
        return {
          prompts: ['Review the changed files.', 'Explain how the session coordinator works.'],
        };
      case 'projects.scratch':
        return { cwd: `/fixture/home/scratch/new-workspace-${++this.scratchSerial}` };
      case 'llm.complete':
        return {
          text: 'Implement desktop session controls',
          model: p.model ?? 'fixture-model',
          provider: 'fixture',
        };
      case 'sessions.discoverForeign':
        return {
          scan_root: p.from,
          count: 1,
          candidates: [
            {
              status: 'discoverable',
              kind: 'codex',
              path: `${p.from}/session.jsonl`,
              marker: 'session.jsonl',
              importable: true,
            },
          ],
          summary: {
            discoverable: 1,
            importable: 1,
            discoverable_not_importable: 0,
            corrupt: 0,
            rejected: 0,
            total: 1,
          },
        };
      case 'sessions.exportMarkdown': {
        if (typeof p.dest !== 'string' || !isAbsolute(p.dest))
          throw new Error('Export destination must be an absolute path');
        const view = projectEvents(this.eventsForRun(p.runDir));
        const markdown = view.entries
          .filter((entry) => entry.kind === 'user' || entry.kind === 'text')
          .map(
            (entry) =>
              `## ${entry.kind === 'user' ? 'User' : 'Assistant'}\n\n${'text' in entry ? entry.text : ''}\n\n`,
          )
          .join('');
        mkdirSync(dirname(p.dest), { recursive: true, mode: 0o700 });
        writeFileSync(p.dest, markdown, { mode: 0o600 });
        return { dest: p.dest, bytes: Buffer.byteLength(markdown) };
      }
      case 'sessions.trace':
        return {
          session_id: p.runDir.split('/').pop(),
          status: 'exported',
          local_path: p.dest,
          bytes: 2048,
        };
      case 'toggles.get':
        return (this.values['toggles.get'] ??= {
          entries: [{ id: 'tool:read', kind: 'tool', name: 'read', enabled: true }],
          appliesTo: 'newSessions',
          persisted: true,
        });
      case 'toggles.set': {
        const state = this.dispatch('toggles.get', p) as Methods['toggles.get']['result'];
        const item = state.entries.find((e) => e.id === p.id);
        if (!item) throw new Error('Toggle not found');
        item.enabled = p.enabled;
        return state;
      }
      case 'trust.check':
        return {
          supported: false,
          trusted: null,
          status: 'unknown',
          reason: 'Coordinator folder trust enforcement is unavailable.',
        };
      case 'trust.set':
        throw new Error('Coordinator folder trust enforcement is unavailable.');
      case 'app.releaseNotes':
        return {
          version: 'fixture',
          markdown:
            '# Harness\n\nDesktop session controls, workspace review, and skill management.\n',
        };
      case 'app.doctor':
        return {
          noNetworkProbes: true,
          providerExecutionProof: false,
          scope: 'local_configuration',
          checks: [{ name: 'configuration', status: 'ok' }],
          trust: { supported: false, status: 'unknown' },
        };
      case 'app.configSources':
        return {
          primaryPath: '/fixture/home/harness.jsonc',
          layers: [{ path: '/fixture/home/harness.jsonc', kind: 'user' }],
          searched: [],
          runtimeOverrides: null,
          scope: 'configuration',
        };
      default:
        throw new Error(`${method} is not configured in this fixture`);
    }
  }
}
function usage(from: string, to: string, cwd: string): UsageHistory {
  const base: UsageAggregate = {
    key: 'total',
    records: 6,
    sessions: 2,
    inputTokens: 24000,
    cachedInputTokens: 16000,
    cacheWriteTokens: 2000,
    outputTokens: 4200,
    reasoningTokens: 800,
    totalTokens: 46200,
    costUsd: 0.12,
    knownCostUsd: 0.12,
    unpricedRecords: 0,
    lastActive: `${to}T12:00:00Z`,
    providers: [],
    models: [],
  };
  const provider = { ...base, key: 'fixture', provider: 'fixture' };
  const model = { ...base, key: 'fixture-model', model: 'fixture-model', provider: 'fixture' };
  const group = (key: string) => ({ ...base, key, providers: [provider], models: [model] });
  return {
    from,
    to,
    timeZone: 'UTC',
    totals: group('total'),
    byCategory: [group('conversation')],
    byModel: [model],
    byProvider: [provider],
    byProject: [{ ...group(cwd), project: cwd }],
    byDay: [group(to)],
    byMonth: [group(to.slice(0, 7))],
    quality: { providerReportedRecords: 0, modelPricedRecords: 6, unpricedRecords: 0 },
    scan: { scanned: 2, skipped: 0, records: 6, elapsedMs: 12 },
    errors: [],
    ratesAvailable: true,
    cacheSavingsUsd: null,
  };
}
