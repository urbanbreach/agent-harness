from pathlib import Path
import subprocess,json,hashlib
p=Path('crates/harness-tui/src/ui_transcript_selection.rs');original=p.read_bytes()
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--lib','-E','test(diagnostic_ascii_selection_rows_match_previous) | test(selection) | test(hyperlink) | test(clipboard)','--success-output','immediate']
try:
 p.write_bytes(original+b'\n#[cfg(test)]\n#[path = "/tmp/tui-tool-layout/row-parity.rs"]\nmod ascii_diagnostic;\n')
 with Path('/tmp/tui-tool-layout/final-row-parity.log').open('w') as log:
  r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 Path('/tmp/tui-tool-layout/final-row-parity-check.json').write_text(json.dumps({'command':command,'exit_code':r.returncode,'rows_sha256':hashlib.sha256(Path('crates/harness-tui/src/ui_transcript_selection/rows.rs').read_bytes()).hexdigest()},indent=2)+'\n')
 print('final row parity and selection checks',r.returncode,flush=True)
finally:
 p.write_bytes(original)
raise SystemExit(r.returncode)
