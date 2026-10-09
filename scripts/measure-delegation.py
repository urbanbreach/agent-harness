#!/usr/bin/env python3
"""Measure retained child resources and SIGINT cleanup using a local provider."""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
import time
import traceback


def resources(pid):
    status = dict(line.split(':', 1) for line in Path(f'/proc/{pid}/status').read_text().splitlines())
    return {'rss_kib': int(status['VmRSS'].split()[0]),
            'descriptors': len(list(Path(f'/proc/{pid}/fd').iterdir()))}


def measure(binary, count, cancel):
    entered, release = threading.Event(), threading.Event()
    samples, children, errors = [], [], []
    process = None
    with tempfile.TemporaryDirectory(prefix='harness-delegation-') as directory:
        root = Path(directory)

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                try:
                    request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    last = request['messages'][-1]
                    child = last.get('content', '').startswith('child work ')
                    if child and cancel:
                        entered.set()
                        release.wait(10)
                        return
                    if child:
                        delta = {'content': 'child report'}
                    else:
                        if last['role'] == 'tool':
                            result = last['content']
                            assert result.startswith('child report\n'), result
                            child_id = next(line.removeprefix('subagent_id: ') for line in result.splitlines()
                                            if line.startswith('subagent_id: '))
                            assert child_id not in children
                            children.append(child_id)
                        if len(children) in {0, 1, 16, 128, count}:
                            samples.append({'children': len(children), **resources(process.pid)})
                        if len(children) == count:
                            delta = {'content': 'delegation complete'}
                        else:
                            args = {'prompt': f'child work {len(children)}',
                                    'description': 'Resource measurement', 'tools': [], 'background': False}
                            delta = {'tool_calls': [{'index': 0, 'id': f'call-{len(children)}', 'type': 'function',
                                      'function': {'name': 'spawn_subagent', 'arguments': json.dumps(args)}}]}
                    self.send_response(200)
                    self.send_header('Content-Type', 'text/event-stream')
                    self.send_header('Connection', 'close')
                    self.end_headers()
                    self.wfile.write(('data: ' + json.dumps({'choices': [{'index': 0, 'delta': delta}]}) + '\n\n').encode())
                    self.wfile.write(b'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n')
                    self.wfile.flush()
                except Exception:
                    errors.append(traceback.format_exc())

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.daemon_threads = True
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        config = root / 'config.json'
        config.write_text(json.dumps({
            'provider': {'local': {'type': 'openai_compatible', 'apiMode': 'chat_completions',
                'baseURL': f'http://127.0.0.1:{server.server_port}/v1', 'apiKeyEnv': [],
                'models': {'fixture': {'limit': {'context': 2000000, 'output': 1000}}}}},
            'model': 'local/fixture',
            'agent': {'default': {'tools': ['spawn_subagent'], 'max_iters': count + 1}},
            'runtime': {'prompt': {'wait_timeout_ms': 180000}, 'compaction': {'enabled': False},
                        'provider_retry': {'max_retries': 0}}
        }))
        environment = {key: os.environ[key] for key in ('PATH', 'LANG', 'LD_LIBRARY_PATH') if key in os.environ}
        environment.update(HOME=str(root / 'home'), HARNESS_HOME=str(root / 'harness-home'))
        started = time.monotonic()
        process = subprocess.Popen([str(binary), '--cwd', str(root), '--config', str(config),
                                    'prompt', '--text', 'launch delegations', '--yolo'],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment, cwd=root)
        try:
            if cancel:
                if not entered.wait(15):
                    raise RuntimeError('child provider never started')
                started = time.monotonic()
                process.send_signal(signal.SIGINT)
            output, error = process.communicate(timeout=5 if cancel else 180)
            elapsed = (time.monotonic() - started) * 1000
            assert process.returncode == (1 if cancel else 0), (error.decode(errors='replace'), errors)
            assert not errors, errors
            if not cancel:
                assert output == b'delegation complete\n', output
                assert len(children) == count
            journals = list((root / 'harness-home/sessions').glob('*/*/events.jsonl'))
            root_journal, = [p for p in journals
                             if not json.loads((p.parent / 'meta.json').read_text()).get('harness_lineage')]
            events = [json.loads(line)['payload']['event_type'] for line in root_journal.read_text().splitlines()]
            assert events[-1] == ('run_failed' if cancel else 'run_finished'), events[-1]
            assert events.count('agent_spawned') == (2 if cancel else count + 1)
            for journal in journals:
                if journal == root_journal:
                    continue
                payloads = [json.loads(line)['payload'] for line in journal.read_text().splitlines()]
                assert any(p['event_type'] == ('task_cancelled' if cancel else 'task_completed')
                           for p in payloads), payloads
                assert payloads[-1]['event_type'] == 'native_subagent_receipt', payloads[-1]
                assert payloads[-1]['data']['kind'] == 'terminal_published', payloads[-1]
            return {'cancelled': cancel, 'children': 1 if cancel else count, 'elapsed_ms': elapsed,
                    'resources': samples, 'journal_bytes': sum(p.stat().st_size for p in journals),
                    'root_events': len(events), 'journals': len(journals)}
        finally:
            release.set()
            if process.poll() is None:
                process.kill()
                process.wait()
            server.shutdown()
            server.server_close()
            worker.join()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--children', type=int, default=256)
    args = parser.parse_args()
    if not 1 <= args.children <= 1024:
        parser.error('--children must be between 1 and 1024')
    binary = args.binary.resolve(strict=True)
    report = {'schema_version': 1, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'delegation': measure(binary, args.children, False), 'cancellation': measure(binary, 1, True)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    samples = report['delegation']['resources']
    assert samples[-1]['descriptors'] <= samples[0]['descriptors'] + 4, samples


if __name__ == '__main__':
    main()
