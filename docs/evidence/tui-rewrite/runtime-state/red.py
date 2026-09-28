from pathlib import Path
import subprocess,json
out=Path(__file__).resolve().parent
source=Path('crates/harness-tui/src/view_model.rs');original=source.read_text()
assert original==(out/'legacy-view_model.rs').read_text()
mutated=original.replace('tool_call.effective_tool_id()', 'tool_call.tool_id')
assert mutated!=original
try:
 source.write_text(mutated)
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(source)]))
 command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--lib','-E','test(live_status_strip_distinguishes_terminal_states) | test(runtime_state_overlay_never_stacks_over_permission_modal)']
 with (out/'red.log').open('w') as log:
  result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode,'mutation':'Display invoked tool ID instead of effective tool ID'},indent=2)+'\n')
 assert result.returncode==100
 assert 'tool queued · task' in (out/'red.log').read_text()
finally:
 source.write_text(original)
