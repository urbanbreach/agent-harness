from pathlib import Path
import json,os,subprocess
root=Path('/home/urbanbreach/Projects/agent-harness')
checks=[
 ('all',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features']),
 ('clippy',['cargo','clippy','-p','harness-tui','--all-targets','--all-features','--','-D','warnings']),
 ('workspace',['cargo','check','--workspace']),
 ('fmt',['cargo','fmt','--all','--','--check']),
 ('gates',['python3','scripts/check-test-suite-gates.py']),
 ('frames',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','rewrite_reference_test']),
 ('motion',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','grok_parity_render_test','-E','test(chat_and_tool_bullets_animate_without_recoloring_labels_or_reflowing_text) | test(all_tool_families_keep_grok_columns_through_streaming_and_disclosure)']),
 ('pty',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--ignore-default-filter','-E','binary(p0_03_pty_recorded) | binary(p1_04_pty_recorded)']),
 ('performance-build',['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--list-type','binaries-only','--message-format','json']),
 ('probes-build',['cargo','build','--release','-p','harness-tui','--all-features','--example','rewrite_probe']),
]
env={**os.environ,'HARNESS_TUI_REFERENCE_FRAMES':'/tmp/tui-tool-layout-matrix','HARNESS_TUI_PLAN_FRAMES':'/tmp/tui-tool-layout-plans','HARNESS_TUI_WRAP_FRAMES':'/tmp/tui-tool-layout-wrap','HARNESS_PARITY_RENDER_ARTIFACT_DIR':'/tmp/tui-tool-layout-motion'}
results=[]
for name,command in checks:
 e={**(env if name in ('frames','motion') else os.environ)}
 if name=='pty':e['HARNESS_TUI_PTY_SIGNOFF']='1'
 with open('/tmp/tui-tool-layout-final-'+name+'.log','w') as log:
  if name=='performance-build':
   with open('/tmp/tui-tool-layout-candidate-binaries.json','w') as metadata:
    r=subprocess.run(command,cwd=root,env=e,stdout=metadata,stderr=log)
  else:
   r=subprocess.run(command,cwd=root,env=e,stdout=log,stderr=subprocess.STDOUT)
 results.append({'name':name,'command':command,'exit_code':r.returncode})
 Path('/tmp/tui-tool-layout-checks.json').write_text(json.dumps(results,indent=2)+'\n')
 print(name,r.returncode,flush=True)
 if r.returncode:raise SystemExit(r.returncode)
