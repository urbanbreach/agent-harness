from pathlib import Path
import hashlib,json,subprocess
out=Path(__file__).resolve().parent
legacy=(out/'legacy-keybindings.rs').read_text()
start=legacy.index('fn format_key_binding(')
end=legacy.index('\nfn parse_leader_sequence',start)
test=Path('crates/harness-tui/tests/rewrite_key_labels_oracle.rs')
assert not test.exists()
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','rewrite_key_labels_oracle','--success-output','immediate']
try:
 test.write_text((out/'oracle-cases.rs').read_text()+'\n'+legacy[start:end])
 with (out/'oracle.log').open('w') as log:result=subprocess.run(command,stdout=log,stderr=log)
 (out/'oracle.json').write_text(json.dumps({'command':command,'exit':result.returncode,'source_sha256':hashlib.sha256(test.read_bytes()).hexdigest()},indent=2)+'\n')
 result.check_returncode()
finally:test.unlink()
