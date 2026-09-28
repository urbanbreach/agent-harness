from pathlib import Path
import hashlib,json,os,subprocess,tempfile
out=Path(__file__).resolve().parent
sources=json.loads((out/'base.json').read_text())['sources']
original={Path(r['path']):Path(r['path']).read_bytes() for r in sources}
command=['cargo','nextest','run','--profile','ci','-p','harness','--all-features','--no-fail-fast','-E','test(project_config_tui_ignores_legacy_model_selection) | test(no_config_tui_ignores_legacy_builtin_model_selection) | test(tui_mock_mode_still_boots_through_launcher)']
try:
 for r in sources:
  p=Path(r['path']);data=(out/'legacy'/p.relative_to('crates/harness-tui/src')).read_bytes();assert hashlib.sha256(data).hexdigest()==r['sha256'];p.write_bytes(data)
 with tempfile.TemporaryDirectory(prefix='tui-dock-baseline-config-') as config:
  with (out/'baseline-failures.log').open('w') as log:
   result=subprocess.run(command,env={**os.environ,'XDG_CONFIG_HOME':config},stdout=log,stderr=subprocess.STDOUT)
 (out/'baseline-failures.json').write_text(json.dumps({'base':json.loads((out/'base.json').read_text())['base'],'command':command,'environment':{'XDG_CONFIG_HOME':'fresh empty temporary directory','HOME':'unchanged'},'exit':result.returncode,'production_sources':[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in original]},indent=2)+'\n')
 assert result.returncode==100
finally:
 for p,data in original.items():p.write_bytes(data)
