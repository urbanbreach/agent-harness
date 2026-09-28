from pathlib import Path
import datetime, json, os, re, statistics, subprocess, sys

root = Path.cwd()
out = Path(__file__).resolve().parent
sides = sys.argv[1:]
assert sides and set(sides) <= {'before', 'candidate'}
order = []
for scenario in os.environ.get('UNDO_JOURNAL_SCENARIOS', 'typing-long,typing,undo-long,delete-long').split(','):
    for sample in ['1', '2', '3', 'alloc']:
        for side in reversed(sides) if sample == '2' else sides:
            folder = out / side
            command = ['cargo', 'nextest', 'run', '--binaries-metadata', str(folder / 'binaries.json'),
                       '--cargo-metadata', str(out / 'cargo.json'), '--profile', 'perf', '-j1',
                       '--success-output', 'immediate']
            if sample == 'alloc':
                command = ['memusage', '-n', f'{side}.bin', '--no-timer', *command]
            stem = f'{scenario}-{sample}'
            env = {**os.environ, 'HARNESS_REWRITE_SCENARIO': scenario, 'HARNESS_REWRITE_HISTORY': '0',
                   'HARNESS_REWRITE_FRAMES': '500', 'HARNESS_REWRITE_PERF_OUT': str(folder / f'{stem}.json')}
            start = datetime.datetime.now(datetime.timezone.utc).isoformat()
            with (folder / f'{stem}.log').open('w') as log:
                result = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
            order.append({'side': side, 'sample': stem, 'started_at': start, 'command': command, 'exit': result.returncode})
            (out / 'run-order.json').write_text(json.dumps(order, indent=2) + '\n')
            print(side, stem, result.returncode, flush=True)
            result.check_returncode()
summaries = {}
for side in sides:
    folder = out / side
    summaries[side] = {}
    for scenario in os.environ.get('UNDO_JOURNAL_SCENARIOS', 'typing-long,typing,undo-long,delete-long').split(','):
        samples = [json.loads((folder / f'{scenario}-{i}.json').read_text()) for i in range(1, 4)]
        summary = {key: statistics.median(s[key] for s in samples) for key in ['p50_us', 'p95_us', 'p99_us', 'bytes']}
        summary['rss_kib'] = statistics.median(s['after']['rss_kib'] for s in samples)
        summary['cpu_ms_per_frame'] = statistics.median((s['after']['cpu_ticks'] - s['before']['cpu_ticks']) * 1000 / os.sysconf('SC_CLK_TCK') / s['frames'] for s in samples)
        log = (folder / f'{scenario}-alloc.log').read_text()
        heap = re.search(r'heap total:\s*(\d+), heap peak:\s*(\d+)', log)
        calls = re.search(r'\n\s*malloc\|\s*(\d+)\s+', log)
        assert heap and calls, log[-3000:]
        summary['allocated_bytes'], summary['peak_heap_bytes'] = map(int, heap.groups())
        summary['malloc_calls'] = int(calls[1])
        summaries[side][scenario] = summary
(out / 'summary.json').write_text(json.dumps(summaries, indent=2) + '\n')
print(json.dumps(summaries, indent=2))
