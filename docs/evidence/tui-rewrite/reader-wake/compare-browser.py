from pathlib import Path
import copy,hashlib,json,math,subprocess
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-reader-wake');base=root/'.omo/evidence/tui-rewrite/reader-wake'
paths=[base/f'{side}-{i}' for i in range(1,4) for side in ('before','candidate')]
reports=[json.loads((p/'latency.json').read_text()) for p in paths]
checks=[]
for name in sorted(p.name for p in paths[0].glob('*.png')):
 hashes=[hashlib.sha256((p/name).read_bytes()).hexdigest() for p in paths]
 checks.append(dict(image=name,equal_across_six_runs=len(set(hashes))==1,sha256=hashes))
 assert len(set(hashes))==1 or (name=='resized.png' and all(h==hashes[0] for i,h in enumerate(hashes) if i!=2))
terminals=[r['emulator']['terminal'] for r in reports]
fields=['cols','rows','title','titleHistory','activeBuffer','cursor','modes','text','cells','wrappedRows','scrollback','lines']
fields_equal={field:all(t[field]==terminals[0][field] for t in terminals) for field in fields}
# Inspect the sole elapsed-clock difference, retaining the unmodified records.
canonical={k:terminals[0][k] for k in fields}
observed={k:copy.deepcopy(terminals[2][k]) for k in fields}
assert set(k for k,v in fields_equal.items() if not v)=={'text','cells','scrollback','lines'}
assert all({k:t[k] for k in fields}==canonical for i,t in enumerate(terminals) if i!=2)
row=observed['lines'][32];assert row['row']==32
assert row['text'].count('53s')==2 and canonical['lines'][32]['text'].count('54s')==2
for k in ('text',):
 assert observed[k].count('53s')==2
 observed[k]=observed[k].replace('53s','54s')
assert observed['scrollback']['text'].count('53s')==2
observed['scrollback']['text']=observed['scrollback']['text'].replace('53s','54s')
row['text']=row['text'].replace('53s','54s')
changed=[]
for cells in (observed['cells'],row['cells']):
 for cell in cells:
  if cell.get('row',32)==32 and cell['column'] in (19,116):
   assert cell['chars']=='3';cell['chars']='4';changed.append(cell['column'])
assert sorted(changed)==[19,19,116,116]
assert observed==canonical,'difference beyond the two elapsed-clock digits'
# Pixel differences must be inside those same two display cells, not elsewhere.
a=subprocess.check_output(['magick',str(paths[2]/'resized.png'),'-depth','8','rgba:-'])
b=subprocess.check_output(['magick',str(paths[0]/'resized.png'),'-depth','8','rgba:-'])
assert len(a)==len(b)==1152*800*4
pixels=[]
for pos in range(0,len(a),4):
 if a[pos:pos+4]!=b[pos:pos+4]:
  pixel=pos//4;x,y=pixel%1152,pixel//1152;pixels.append([x,y])
  assert 640<=y<660 and any(math.floor(col*9.6)<=x<math.ceil((col+1)*9.6) for col in (19,116)),(x,y)
comparison=dict(screenshots=checks,raw_terminal_fields_equal=fields_equal,
 elapsed_clock_difference={'run':'before-2','row_zero_based':32,'columns_zero_based':[19,116],'before_text':'53s','other_runs_text':'54s','all_other_terminal_properties_equal':True,'different_pixels':len(pixels),'pixel_bounds':[min(x for x,y in pixels),min(y for x,y in pixels),max(x for x,y in pixels)+1,max(y for x,y in pixels)+1]},
 observers=[dict(run=p.name,parsed_count=r['emulator']['terminal']['parsedCount'],output_bytes=r['output_bytes']) for p,r in zip(paths,reports)],
 cleanup=[dict(exit=r['exit'],termios_restored=r['termios_restored'],protocol_restored=r['protocol_restored'],cleanup=r['cleanup'],browser_cleanup=r['browser_cleanup'],temporary_root_removed=r['temporary_root_removed']) for r in reports])
for r in reports:
 assert r['exit']['code']==0 and r['exit']['signal'] is None and r['termios_restored'] and r['protocol_restored'] and r['temporary_root_removed']
 assert r['cleanup']['childExited'] and not r['cleanup']['processGroupAlive'] and r['cleanup']['stdinClosed'] and not r['cleanup']['terminatedByCleanup'] and not r['cleanup']['temporarySockets']
 assert r['browser_cleanup']['pageClosed'] and r['browser_cleanup']['contextClosed'] and not r['browser_cleanup']['browserConnectedAfterClose'] and r['browser_cleanup']['profileRemoved'] and not r['browser_cleanup']['boundPorts']
(out/'browser-comparisons.json').write_text(json.dumps(comparison,indent=2)+'\n');print(comparison['elapsed_clock_difference'])
