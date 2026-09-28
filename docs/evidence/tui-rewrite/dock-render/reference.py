from pathlib import Path
import hashlib,json,subprocess,sys
out=Path(__file__).resolve().parent
sources=json.loads((out/'base.json').read_text())['sources']
original={Path(r['path']):Path(r['path']).read_bytes() for r in sources}
try:
 for r in sources:
  p=Path(r['path']);data=(out/'legacy'/p.relative_to('crates/harness-tui/src')).read_bytes();assert hashlib.sha256(data).hexdigest()==r['sha256'];p.write_bytes(data)
 command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--lib','-E','test(live_composer_disclosure_keeps_compact_summary_and_commands) | test(runtime_context_labels_distinguish_live_continue_and_replay) | test(live_switch_model_labels_next_turn_only) | test(cache_read_write_tokens_render_as_separate_status_labels)']
 with (out/'public-behavior-before.log').open('w') as log:r=subprocess.run(command,stdout=log,stderr=log)
 (out/'public-behavior-before.json').write_text(json.dumps({'command':command,'exit':r.returncode,'production_sources':[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in original]},indent=2)+'\n');r.check_returncode()
 subprocess.run(['python3',str(out/'red.py')] if '--red' in sys.argv else ['python3',str(out/'oracle.py'),'before'],check=True)
finally:
 for p,data in original.items():p.write_bytes(data)
