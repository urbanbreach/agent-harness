from pathlib import Path
import json, os, subprocess, tempfile
out=Path(__file__).resolve().parent
command=['cargo','nextest','run','--profile','ci','--workspace','--all-features','--no-fail-fast']
with tempfile.TemporaryDirectory(prefix='tui-dock-workspace-config-') as config:
 with (out/'workspace-tests.log').open('w') as log:
  result=subprocess.run(command,env={**os.environ,'XDG_CONFIG_HOME':config},stdout=log,stderr=subprocess.STDOUT)
(out/'workspace-tests.json').write_text(json.dumps({'command':command,'environment':{'XDG_CONFIG_HOME':'fresh empty temporary directory','HOME':'unchanged'},'exit':result.returncode},indent=2)+'\n')
print('workspace tests',result.returncode,flush=True)
