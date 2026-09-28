from pathlib import Path
import gzip,hashlib,json,platform,shutil,subprocess,tarfile
out=Path(__file__).resolve().parent;dest=Path('docs/evidence/tui-rewrite/dock-render');dest.mkdir(exist_ok=True)
for name in ['acceptance-before-implementation.json','base.json','before-reuse-origin.json','sources.json','summary.json','comparison.json','oracle-comparison.json','checks.json','workspace-tests.json','workspace-failure-comparison.json','baseline-failures.json','dock-final-runs.json','validation.json','public-behavior-before.json','red.json','allocation-summary.json','build.py','checks.py','workspace.py','baseline-failures.py','dock-final.py','finish.py','measure.py','compare.py','browser.py','oracle.py','oracle.rs','reference.py','nextest-oracle.toml','red.py','prepare.py','freeze.py','publish.py','profile.py','profile.gdb','profile-runner.py','profile-summary.py']:
 shutil.copy2(out/name,dest/name)
for pattern in ['capture-*.mjs','compare-*.py','browser-*-comparison.json','oracle-*-run.json','oracle-*.jsonl.gz']:
 for p in out.glob(pattern):shutil.copy2(p,dest/p.name)
base=json.loads((out/'base.json').read_text())['base']
patch=subprocess.check_output(['git','diff',base,'--','crates/harness-tui/src'])
for path in subprocess.check_output(['git','ls-files','--others','--exclude-standard','crates/harness-tui/src']).decode().splitlines():
 r=subprocess.run(['git','diff','--no-index','--','/dev/null',path],capture_output=True);assert r.returncode==1;patch+=r.stdout
(dest/'source.patch.gz').write_bytes(gzip.compress(patch,mtime=0))
def archive(name,paths):
 with tarfile.open(dest/name,'w:gz') as tar:
  for p in paths:tar.add(p,arcname=p.relative_to(out))
archive('performance.tar.gz',[out/name for name in ['baseline','before','candidate','cargo.json','run-order.json','measure-run.log','compare-run.log','baseline-before-implementation.tar.gz','build-candidate.log','build-run.log']])
archive('checks.tar.gz',[*out.glob('*.log'),out/'browser-runs.json'])
archive('preparation.tar.gz',[out/'preparation',out/'initial-oracle-differences.json',out/'apply.py',out/'adapt-tests.py',out/'prepare-oracle.py'])
archive('legacy.tar.gz',[out/'legacy'])
archive('profile.tar.gz',[p for p in out.glob('profile*') if p.suffix!='.bin']+[out/'allocation-stacks.log'])
raw=Path('.omo/evidence/tui-rewrite/dock-render')
with tarfile.open(dest/'browser.tar.gz','w:gz') as tar:
 for p in sorted(raw.rglob('*')):
  if p.is_file():tar.add(p,arcname=p.relative_to(raw))
env={'platform':platform.platform(),'rustc':subprocess.check_output(['rustc','--version']).decode().strip(),'cargo':subprocess.check_output(['cargo','--version']).decode().strip(),'release_profile':{'opt_level':3,'lto':'fat','codegen_units':1},'binaries':{}}
for name in ['before.bin','candidate.bin','before-probe','candidate-probe']:
 p=out/name;env['binaries'][name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'bytes':p.stat().st_size}
(dest/'environment.json').write_text(json.dumps(env,indent=2)+'\n')
assert 'frozen_dock_render' not in Path('crates/harness-tui/src/app.rs').read_text()
for row in json.loads((out/'sources.json').read_text())['files']:
 assert hashlib.sha256(Path(row['path']).read_bytes()).hexdigest()==row['sha256']
for journey,count in [('runtime',12),('composer',31),('grouped',11),('layout',16),('dock',13)]:
 r=json.loads((out/f'browser-{journey}-comparison.json').read_text());assert len(r['comparisons'])==count and r['both_runs_restore_and_cleanup'] and all(row['pixels_equal'] for row in r['comparisons'])
rows=[{'path':p.name,'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(dest.iterdir()) if p.is_file() and p.name!='files.json']
(dest/'files.json').write_text(json.dumps(rows,indent=2)+'\n');print(len(rows),'artifacts')
