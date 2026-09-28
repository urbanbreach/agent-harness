from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent;base='03784425760ff676c37d9cb8f487ca03106b6cd2'
paths=[Path(row['path']) for row in json.loads((out/'before/binary.json').read_text())['production_sources']]
current={p:p.read_bytes() if p.exists() else None for p in paths};p=Path('crates/harness-tui/src/ui_composer/file_tags.rs')
source=subprocess.check_output(['git','show',f'{base}:{p}']).decode();assert source.count('style = style.add_modifier(Modifier::REVERSED);')==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','-E','test(composer_selection_and_mentions_follow_visible_wrapped_cells) | test(composer_metadata_) | binary(production_composer_reachability_test)']
try:
 for path in paths:path.write_bytes(subprocess.check_output(['git','show',f'{base}:{path}']))
 p.write_text(source.replace('style = style.add_modifier(Modifier::REVERSED);','style = Style::default().add_modifier(Modifier::REVERSED);'))
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)]))
 with (out/'red.log').open('w') as log:result=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n');assert result.returncode==100
finally:
 for path,data in current.items():
  if data is None:path.unlink(missing_ok=True)
  else:path.write_bytes(data)
