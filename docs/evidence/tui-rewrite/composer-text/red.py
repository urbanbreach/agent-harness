from pathlib import Path
import subprocess
out=Path(__file__).resolve().parent
source=Path('crates/harness-tui/src/composer_atoms/buffer.rs')
before=source.read_text()
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','composer_atoms_test']
with (out/'before-check.log').open('w') as log:
    subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,check=True)
start=before.index('    pub fn text(&self) -> String {')
end=before.index('\n    pub fn insert_text_at(', start)
try:
    source.write_text(before[:start]+'    pub fn text(&self) -> String {\n        String::new()\n    }\n'+before[end:])
    (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(source)]))
    with (out/'red.log').open('w') as log:
        result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
    assert result.returncode==100,result.returncode
finally:
    source.write_text(before)
print('baseline passed; empty text projection failed; original restored',flush=True)
