from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent
p=Path('crates/harness-tui/src/ui_composer/presentation.rs')
paths=[Path(name) for name in ['crates/harness-tui/src/app/composer.rs', 'crates/harness-tui/src/composer_integration/mod.rs', 'crates/harness-tui/src/composer_integration/presentation.rs', 'crates/harness-tui/src/composer_integration/presentation_policy.rs', 'crates/harness-tui/src/composer_integration/view_model.rs', 'crates/harness-tui/src/ui_composer/bordered.rs', 'crates/harness-tui/src/ui_composer/collapsed.rs', 'crates/harness-tui/src/ui_composer/document.rs', 'crates/harness-tui/src/ui_composer/presentation.rs']]
current={path:path.read_bytes() for path in paths}
source=subprocess.check_output(['git','show','16ab0230547d73b0b9468eab683c880b920cda47:'+str(p)]).decode()
assert source.count('if actual.text() == text {')==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','production_composer_reachability_test','--test','ghost_suggestion_render_test']
try:
 for path in paths:path.write_bytes(subprocess.check_output(['git','show','16ab0230547d73b0b9468eab683c880b920cda47:'+str(path)]))
 p.write_text(source.replace('if actual.text() == text {','if actual.text() == text && text.is_empty() {'))
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)]))
 with (out/'red.log').open('w') as log: result=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n')
 assert result.returncode==100
finally:
 for path,data in current.items():path.write_bytes(data)
