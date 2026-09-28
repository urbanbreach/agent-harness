from pathlib import Path
import gzip,hashlib,json,platform,shutil,subprocess,tarfile
out=Path(__file__).resolve().parent;dest=Path('docs/evidence/tui-rewrite/frame-layout');dest.mkdir(exist_ok=True)
for p in sorted(out.iterdir()):
 if p.is_file() and p.suffix in ['.py','.mjs','.json','.log','.toml','.rs'] and not p.name.startswith(('preparation-','initial-','partition-probe')):
  shutil.copy2(p,dest/p.name)
shutil.copy2(out/'allocation-diagnostic.log.gz',dest/'allocation-diagnostic.log.gz')
base=json.loads((out/'base.json').read_text())['base']
patch=subprocess.check_output(['git','diff',base,'--','crates/harness-tui/src'])
for path in subprocess.check_output(['git','ls-files','--others','--exclude-standard','crates/harness-tui/src']).decode().splitlines():
 r=subprocess.run(['git','diff','--no-index','--','/dev/null',path],capture_output=True);assert r.returncode==1;patch+=r.stdout
(dest/'source.patch.gz').write_bytes(gzip.compress(patch,mtime=0))
with tarfile.open(dest/'performance.tar.gz','w:gz') as tar:
 for name in ['before','candidate','baseline','cargo.json','build-candidate.log','run-order.json','measure-run.log','before-reuse-origin.json']:tar.add(out/name,arcname=name)
with tarfile.open(dest/'preparation.tar.gz','w:gz') as tar:
 for pattern in ['preparation-*','initial-*','partition-probe.rs','partition-probe.log']:
  for p in sorted(out.glob(pattern)):tar.add(p,arcname=p.name)
 if (out/'preparation').exists():tar.add(out/'preparation',arcname='preparation')
with tarfile.open(dest/'legacy.tar.gz','w:gz') as tar:tar.add(out/'legacy',arcname='legacy')
raw=Path('.omo/evidence/tui-rewrite/frame-layout')
with tarfile.open(dest/'browser.tar.gz','w:gz') as tar:
 for p in sorted(raw.rglob('*')):
  if p.is_file():tar.add(p,arcname=p.relative_to(raw))
env={'platform':platform.platform(),'rustc':subprocess.check_output(['rustc','--version']).decode().strip(),'cargo':subprocess.check_output(['cargo','--version']).decode().strip(),'release_profile':{'opt_level':3,'lto':'fat','codegen_units':1},'binaries':{}}
for name in ['before.bin','candidate.bin','before-probe','candidate-probe']:
 p=out/name;env['binaries'][name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'bytes':p.stat().st_size}
(dest/'environment.json').write_text(json.dumps(env,indent=2)+'\n')
comparison=json.loads((out/'comparison.json').read_text());checks=[c for group in comparison['checks'].values() for c in group.values()]
for journey,count in [('runtime',12),('composer',31),('grouped',11),('layout',16)]:
 r=json.loads((out/f'browser-{journey}-comparison.json').read_text());assert len(r['comparisons'])==count and r['both_runs_restore_and_cleanup'] and all(row['pixels_equal'] for row in r['comparisons'])
verification={'tui':{'run':1634,'passed':1634,'skipped':7},'pty':{'run':7,'passed':7},'public_tests_before':{'run':2,'passed':2},'mutation':{'run':2,'passed':1,'failed':1},'differential':{'cases':75168,'equal':75168,'states':29,'themes':3,'rectangles':864,'temporary_registration_removed':True,'initial_predecessor_and_candidate_cases':51840,'review_and_rewind_states_added_during_review':9},'workspace_check':'passed','clippy':'passed','fmt':'passed','suite_gates':'passed','performance_runs_retained':48,'performance_limits':{'run':len(checks),'passed':sum(c['pass'] for c in checks)},'paired_content_checks':16,'paired_browser_frames':70,'browser_exact_pngs':True,'browser_exact_terminal_state_except_observer_counts':True,'terminal_and_resource_cleanup':'all eight final runs passed','workspace_test_suite':'not rerun; earlier predecessor CLI fixture failures remain recorded','whole_rewrite':'unfinished'}
assert 'frozen_frame_layout' not in Path('crates/harness-tui/src/lib.rs').read_text()
(dest/'verification.json').write_text(json.dumps(verification,indent=2)+'\n')
rows=[{'path':p.name,'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(dest.iterdir()) if p.is_file() and p.name!='files.json']
(dest/'files.json').write_text(json.dumps(rows,indent=2)+'\n');print(len(rows),'artifacts')
