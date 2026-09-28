from pathlib import Path
import hashlib,json,subprocess,sys
out=Path(__file__).resolve().parent
legacy=(out/'legacy/layout.rs').read_text();legacy=legacy[:legacy.index('#[cfg(test)]\n#[path = "layout_live_dock_test_fixtures.rs"]')]
# Preserve the producer exactly; only module paths and test registration differ.
for name in ['overlays','permission','surfaces']:legacy=legacy.replace(f'mod {name};',f'#[path = "{out}/legacy/layout/{name}.rs"]\nmod {name};')
legacy+='\n#[path = "'+str(out)+'/legacy/layout_live_dock_test_fixtures.rs"]\nmod live_dock_test_fixtures;\ninclude!("'+str(out)+'/oracle-cases.rs");\n'
(out/'oracle.rs').write_text(legacy)
p=Path('crates/harness-tui/src/lib.rs');original=p.read_bytes()
command=['cargo','nextest','run','--config-file',str(out/'nextest-oracle.toml'),'--profile','ci','-p','harness-tui','--lib','-E','test(frozen_frame_plan_matches_replacement)','--success-output','immediate']
label=sys.argv[1] if len(sys.argv)>1 else 'oracle'
try:
 p.write_bytes(original+f'\n#[cfg(test)]\n#[path = "{out}/oracle.rs"]\nmod frozen_frame_layout;\n'.encode())
 with (out/f'{label}.log').open('w') as log:r=subprocess.run(command,stdout=log,stderr=log)
 (out/f'{label}.json').write_text(json.dumps({'command':command,'exit':r.returncode,'oracle_sha256':hashlib.sha256((out/'oracle.rs').read_bytes()).hexdigest(),'cases_sha256':hashlib.sha256((out/'oracle-cases.rs').read_bytes()).hexdigest()},indent=2)+'\n');r.check_returncode()
finally:p.write_bytes(original)
