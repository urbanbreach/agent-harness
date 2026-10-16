import { describe, expect, test } from 'bun:test';
import { presentTool } from '../../src/model/tools';

describe('tool presentation', () => {
  test('classifies tool activity independently of translated labels', () => {
    const cases = [
      {
        id: 'bash',
        args: { command: 'cargo check', description: 'Check workspace' },
        status: 'running',
        kind: 'command',
        target: 'cargo check',
      },
      {
        id: 'fs.read',
        args: { filePath: 'src/main.rs' },
        status: 'succeeded',
        kind: 'read',
        target: 'src/main.rs',
      },
      {
        id: 'grep',
        args: { pattern: 'needle', include: '*.rs' },
        status: 'failed',
        kind: 'search',
        target: 'needle',
      },
      {
        id: 'websearch',
        args: { query: 'Rust ownership' },
        status: 'succeeded',
        kind: 'web',
        target: 'Rust ownership',
      },
      {
        id: 'todowrite',
        args: {},
        status: 'running',
        kind: 'todo',
      },
      {
        id: 'question',
        args: { questions: [{ question: 'Which branch?' }] },
        status: 'running',
        kind: 'question',
      },
      {
        id: 'extension.inspectState',
        args: {},
        status: 'succeeded',
        kind: 'generic',
      },
    ];
    for (const { id, args, status, kind, target } of cases) {
      expect(presentTool(id, args, undefined, { status })).toMatchObject({ kind, status, target });
    }
  });

  test('retains command streams and search results without manufacturing missing totals', () => {
    expect(
      presentTool(
        'bash',
        { command: 'false', cwd: '/workspace' },
        {
          stdout: 'hello\n',
          stderr: 'failure\n',
          exit_code: 1,
          is_error: true,
          truncated: true,
        },
      ),
    ).toMatchObject({
      status: 'failed',
      output: {
        kind: 'command',
        command: 'false',
        cwd: '/workspace',
        stdout: 'hello\n',
        stderr: 'failure\n',
        exitCode: 1,
        truncated: true,
      },
    });
    const cases = [
      {
        args: {},
        out: {
          matches: ['a.rs:7:  needle', { path: 'a.rs', line_number: 9, text: 'needle again' }],
          total_count: 2,
        },
        expected: {
          mode: 'content',
          groups: [
            {
              path: 'a.rs',
              rows: [
                { line: 7, text: '  needle' },
                { line: 9, text: 'needle again' },
              ],
            },
          ],
          total: 2,
        },
      },
      {
        args: { output_mode: 'count' },
        out: { files: [{ path: 'a.rs', count: 4 }], total_count: null, truncated: true },
        expected: {
          mode: 'count',
          groups: [{ path: 'a.rs', rows: [{ text: '', count: 4 }] }],
          total: undefined,
          truncated: true,
        },
      },
      {
        args: { output_mode: 'files_with_matches' },
        out: { files: [{ path: 'a.rs', count: 4 }] },
        expected: { mode: 'files_with_matches', groups: [{ path: 'a.rs', rows: [] }] },
      },
    ];
    for (const { args, out, expected } of cases)
      expect(presentTool('grep', args, out).output).toMatchObject({ kind: 'search', ...expected });
  });

  test('preserves edit artifacts, inline content, and failed-edit output', () => {
    const patch = presentTool(
      'apply_patch',
      {},
      {
        edits: [
          { path: 'src/a.rs', diff_rel_path: 'artifacts/a.diff', additions: 2, deletions: 1 },
          { path: 'src/b.rs', diff_rel_path: 'artifacts/b.diff' },
        ],
      },
    );
    expect(patch).toMatchObject({
      kind: 'edit',
      title: 'Edited 2 files',
      detail: '2 files',
      output: {
        kind: 'diff',
        files: [
          { path: 'src/a.rs', artifact: 'artifacts/a.diff', additions: 2, deletions: 1 },
          { path: 'src/b.rs', artifact: 'artifacts/b.diff' },
        ],
      },
    });
    expect(
      presentTool(
        'edit',
        { filePath: 'a.rs', oldString: 'before', newString: 'after' },
        'Permission denied',
        { status: 'failed' },
      ),
    ).toMatchObject({
      title: 'Failed to edit a.rs',
      output: {
        kind: 'diff',
        files: [{ path: 'a.rs', before: 'before', after: 'after' }],
        error: 'Permission denied',
      },
    });
  });

  test('projects web citations and MCP content while preserving generic structured output', () => {
    expect(
      presentTool(
        'webfetch',
        { url: 'https://example.test' },
        {
          text: 'Page',
          sources: ['https://one.test', { url: 'https://two.test', title: 'Two' }],
          status: 200,
        },
      ),
    ).toMatchObject({
      output: {
        kind: 'web',
        text: 'Page',
        sources: [{ url: 'https://one.test' }, { url: 'https://two.test', title: 'Two' }],
        metadata: { status: 200 },
      },
    });
    expect(
      presentTool(
        'mcp.catalog.tool.call',
        { tool: 'find_package', arguments: { name: 'react' } },
        {
          server: { id: 'catalog' },
          payload: {
            tool: 'find_package',
            result: {
              content: [{ type: 'text', text: 'First' }, { resource: { text: 'Second' } }],
            },
          },
        },
      ),
    ).toMatchObject({
      kind: 'mcp',
      title: 'Catalog Find Package',
      output: {
        kind: 'mcp',
        server: 'catalog',
        arguments: { name: 'react' },
        text: 'First\nSecond',
      },
    });
    expect(presentTool('plugin.inspect', null, '{"nested":[1,2]}').output).toEqual({
      kind: 'json',
      value: { nested: [1, 2] },
    });
  });
});
