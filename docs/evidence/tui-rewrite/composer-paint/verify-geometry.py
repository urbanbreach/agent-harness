from pathlib import Path
import hashlib,json,shutil,subprocess
out=Path(__file__).resolve().parent
p=Path('crates/harness-tui/tests/rewrite_paint_probe.rs');assert not p.exists(),p
source=Path('crates/harness-tui/src/ui_composer/file_tags.rs')
head='''use ratatui::{style::{Color,Modifier,Style},text::{Line,Span}};
use harness_tui::UnwrapOrAbort;
// Only the scalar bounds are consumed by the extracted styling functions.
mod app { pub struct FileMentionTag {pub start:usize,pub end:usize} }
'''
def helper(path):return path.read_text().split('pub(super) fn composer_selection')[0]
fixture=head+'mod legacy {\n'+helper(out/'legacy-file_tags.rs')+'}\nmod candidate {\n'+helper(source)+'}\n'+(out/'oracle-cases.rs').read_text()
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','rewrite_paint_probe','--success-output','immediate']
try:
 p.write_text(fixture)
 subprocess.run(['cargo','fmt','--all'],check=True)
 shutil.copy2(p,out/p.name)
 receipt={'command':command,'source_path':str(source),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'fixture_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'frozen_source_sha256':hashlib.sha256((out/'legacy-file_tags.rs').read_bytes()).hexdigest()}
 with (out/'oracle.log').open('w') as log:result=subprocess.run(command,stdout=log,stderr=log)
 receipt['exit']=result.returncode
 (out/'oracle.json').write_text(json.dumps(receipt,indent=2)+'\n');result.check_returncode()
finally:p.unlink(missing_ok=True)
