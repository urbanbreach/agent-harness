from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path('/home/urbanbreach/Projects/agent-harness');output=Path('/tmp/tui-stage-profile');output.mkdir(exist_ok=True)
if subprocess.check_output(['git','status','--porcelain'],cwd=root):raise SystemExit('Require a clean committed source tree before profiling')
p=root/'crates/harness-tui/tests/rewrite_performance_test.rs';original=p.read_bytes();text=original.decode();(output/'original-benchmark.rs').write_bytes(original)
text=text.replace('    let mut samples_us = Vec::with_capacity(frames);','    let mut samples_us = Vec::with_capacity(frames);\n    let mut stage_samples_ns = Vec::with_capacity(frames);')
old='''        j.app.set_frame_area(area);
        terminal.draw(|frame| render_app(frame, &j.app))?;
        if index >= 10 {
            samples_us.push(start.elapsed().as_micros());
        }'''
new='''        let input_elapsed = start.elapsed();
        let prepare_start = Instant::now();
        j.app.set_frame_area(area);
        let prepare_elapsed = prepare_start.elapsed();
        let paint_ns = Cell::new(0u128);
        let draw_start = Instant::now();
        terminal.draw(|frame| {
            let paint_start = Instant::now();
            render_app(frame, &j.app);
            paint_ns.set(paint_start.elapsed().as_nanos());
        })?;
        let draw_elapsed = draw_start.elapsed();
        if index >= 10 {
            samples_us.push(start.elapsed().as_micros());
            stage_samples_ns.push([
                input_elapsed.as_nanos(),
                prepare_elapsed.as_nanos(),
                paint_ns.get(),
                draw_elapsed.as_nanos().saturating_sub(paint_ns.get()),
            ]);
        }'''
assert text.count(old)==1;text=text.replace(old,new)
text=text.replace('"construction_us": construction_us, "cold_us": cold_us, "samples_us": samples_us,','"construction_us": construction_us, "cold_us": cold_us, "samples_us": samples_us,\n        "stage_names": ["input_or_resize", "prepare", "paint", "draw_overhead_diff_encode"],\n        "stage_samples_ns": stage_samples_ns,')
try:
 p.write_text(text)
 (output/'benchmark.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)],cwd=root))
 with (output/'build.log').open('w') as log:
  metadata=json.loads(subprocess.check_output(['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--message-format','json'],cwd=root,stderr=log))
 binary=Path(next(iter(metadata['rust-suites'].values()))['binary-path']);retained=output/'profile.bin';shutil.copy2(binary,retained)
 receipt={'base':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'original_benchmark_sha256':hashlib.sha256(original).hexdigest(),'instrumented_benchmark_sha256':hashlib.sha256(text.encode()).hexdigest(),'binary':{'path':str(retained),'sha256':hashlib.sha256(retained.read_bytes()).hexdigest(),'bytes':retained.stat().st_size},'scope':'single diagnostic run per workload; stage attribution only, excluded from acceptance timing and resource results'}
 (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
 command=['cargo','nextest','run','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--profile','perf','-j1','--success-output','immediate']
 for scenario in ['startup','idle','typing','stream','scroll','resize']:
  env={**os.environ,'HARNESS_REWRITE_SCENARIO':scenario,'HARNESS_REWRITE_HISTORY':'0' if scenario=='startup' else '1000','HARNESS_REWRITE_FRAMES':'200','HARNESS_REWRITE_PERF_OUT':str(output/(scenario+'.json'))}
  with (output/(scenario+'.log')).open('w') as log:subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  result=json.loads((output/(scenario+'.json')).read_text());sums=[sum(row[i] for row in result['stage_samples_ns']) for i in range(4)];total=sum(sums)
  print(scenario,{name:round(value/total*100,1) for name,value in zip(result['stage_names'],sums)},flush=True)
finally:p.write_bytes(original)
print('Original benchmark source restored',flush=True)
