from pathlib import Path
import hashlib,json,subprocess
out=Path(__file__).resolve().parent
frozen=(out/'legacy-view_model.rs').read_text()
oracle=out/'oracle.rs';oracle.write_text(frozen+(out/'oracle-cases.rs').read_text())
lib=Path('crates/harness-tui/src/lib.rs');original=lib.read_text()
try:
 lib.write_text(original+'\n#[cfg(test)]\n#[path = "'+str(oracle)+'"]\nmod frozen_runtime_oracle;\n')
 command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--lib','-E','test(frozen_runtime_projection_matches_replacement)','--success-output','immediate']
 with (out/'oracle.log').open('w') as log:
  result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 (out/'oracle.json').write_text(json.dumps({'command':command,'exit':result.returncode,'reference_sha256':hashlib.sha256(frozen.encode()).hexdigest(),'cases_sha256':hashlib.sha256((out/'oracle-cases.rs').read_bytes()).hexdigest()},indent=2)+'\n')
 result.check_returncode()
finally:
 lib.write_text(original)
