import { mkdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { gitCommand } from '../../../src/workspace/git';

export const panelRepos = '/tmp/harness-panel-repos';
const release = [
  '# Atlas release notes',
  '',
  'A **reliable release** starts with *small changes*, ~~last-minute guesses~~, and `clear ownership`. Read the [release guide](https://example.com/guide).',
  '',
  '## Plan the rollout',
  '',
  '- Prepare the checklist',
  '- Review the changes',
  '  - Confirm owners',
  '  - Check rollback steps',
  '',
  '1. Stage the release',
  '2. Watch the metrics',
  '3. Share the outcome',
  '',
  '- [x] Draft release notes',
  '- [ ] Confirm the launch window',
  '',
  '### Operational checks',
  '',
  '> Ship calmly. Prefer evidence over assumptions.',
  '',
  '| Area | Owner | Checks |',
  '| :--- | :---: | ---: |',
  '| API | Maya | 12 |',
  '| Search | Leo | 8 |',
  '| Docs | Nina | 5 |',
  '',
  '#### Implementation examples',
  '',
  '```rust',
  'fn main() {',
  '    println!("Atlas is ready");',
  '}',
  '```',
  '',
  '```typescript',
  'const release = { name: "atlas", ready: true };',
  'console.log(release.name);',
  '```',
  '',
  '```bash',
  "printf 'Release ready\\n'",
  'git status --short',
  '```',
  '',
  '```',
  'Release checklist complete.',
  'No blocking issues found.',
  '```',
  '',
  '---',
  '',
  'The next step is to confirm the launch window with the team.',
  '',
].join('\n');

/** Rebuilds only the dedicated panel test directory, never a user's workspace. */
export async function seedPanelRepos() {
  await rm(panelRepos, { recursive: true, force: true });
  await mkdir(panelRepos, { recursive: true });
  for (const name of ['atlas', 'harbor']) {
    const cwd = join(panelRepos, name);
    await mkdir(cwd);
    await gitCommand(cwd, ['init', '-b', 'main']);
    await gitCommand(cwd, ['config', 'user.name', 'Reference Lab']);
    await gitCommand(cwd, ['config', 'user.email', 'ref@example.invalid']);
    await writeFile(
      join(cwd, 'README.md'),
      `# ${name === 'atlas' ? 'Atlas' : 'Harbor'}\n\nA reliable place for project notes.\n`,
    );
    await writeFile(
      join(cwd, 'notes.txt'),
      'Release checklist\nReview error handling\nUpdate documentation\n',
    );
    await gitCommand(cwd, ['add', '.']);
    await gitCommand(cwd, ['commit', '-m', 'Initial project notes']);
    await gitCommand(cwd, ['branch', 'feature/search']);
    await gitCommand(cwd, ['switch', '-c', 'feature/release-review']);
    if (name === 'atlas') {
      await rm(join(cwd, 'README.md'));
      await writeFile(
        join(cwd, 'notes.txt'),
        'Release checklist\nReview error handling\nConfirm launch window\nUpdate documentation\n',
      );
      await writeFile(join(cwd, 'main.rs'), 'fn main() {\n    println!("Atlas is ready");\n}\n');
      await writeFile(join(cwd, 'release.md'), release);
      await writeFile(
        join(cwd, 'summary.txt'),
        'Atlas release\nOwner: Maya\nStatus: Ready for review\n',
      );
      const index = join(panelRepos, 'checkpoint-index');
      const env = { GIT_INDEX_FILE: index };
      await gitCommand(cwd, ['read-tree', 'HEAD'], { env });
      const blob = (
        await gitCommand(cwd, ['hash-object', '-w', '--stdin'], {
          input:
            'Release checklist\nReview error handling\nConfirm launch window\nUpdate documentation\n',
        })
      ).stdout.trim();
      await gitCommand(cwd, ['update-index', '--cacheinfo', '100644', blob, 'notes.txt'], { env });
      const tree = (await gitCommand(cwd, ['write-tree'], { env })).stdout.trim();
      const commit = (
        await gitCommand(cwd, ['commit-tree', tree, '-p', 'HEAD', '-m', 'Fixture completed turn'])
      ).stdout.trim();
      await gitCommand(cwd, ['update-ref', 'refs/harness-gui/release-review/1-start', 'HEAD']);
      await gitCommand(cwd, ['update-ref', 'refs/harness-gui/release-review/1-end', commit]);
      await rm(index);
    }
  }
  const remote = join(panelRepos, 'atlas-remote.git');
  await gitCommand(panelRepos, ['init', '--bare', remote]);
  await gitCommand(join(panelRepos, 'atlas'), ['remote', 'add', 'origin', remote]);
  await gitCommand(join(panelRepos, 'atlas'), ['push', '-u', 'origin', 'feature/release-review']);
  return { atlas: join(panelRepos, 'atlas'), harbor: join(panelRepos, 'harbor'), remote };
}
if (import.meta.main) console.log(await seedPanelRepos());
