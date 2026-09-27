from pathlib import Path
import hashlib,json,os,shutil,subprocess,time
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-prepare-profile');out.mkdir(exist_ok=True)
if subprocess.check_output(['git','status','--porcelain'],cwd=root):raise SystemExit('Require clean source')
files=['crates/harness-tui/src/ui_transcript_frame.rs','crates/harness-tui/src/ui_transcript_layout.rs']
original={f:(root/f).read_bytes() for f in files}
try:
 p=root/files[0];s=p.read_text();s=s.replace('        let sections = current.map_or_else(', '        let profile_start = std::time::Instant::now();\n        let sections = current.map_or_else(');s=s.replace('        let previous = self.layouts.iter().rev().find(|entry| {', '        let profile_sections = profile_start.elapsed().as_nanos();\n        let previous = self.layouts.iter().rev().find(|entry| {');s=s.replace('        PreparedLayout {\n            key,', '        eprintln!("PROFILE build {width} {profile_sections} {}", profile_start.elapsed().as_nanos() - profile_sections);\n        PreparedLayout {\n            key,');p.write_text(s)
 p=root/files[1];s=p.read_text();s=s.replace('    let mut top_row = 0;\n    let mut measured_sections', '    let profile_start = std::time::Instant::now();\n    let mut profile_surfaces = 0;\n    let mut profile_text = 0;\n    let mut profile_selection = 0;\n    let mut top_row = 0;\n    let mut measured_sections');s=s.replace('        let surfaces = render_surfaces(section, theme, width, base_surface);', '        let stage = std::time::Instant::now();\n        let surfaces = render_surfaces(section, theme, width, base_surface);\n        profile_surfaces += stage.elapsed().as_nanos();');s=s.replace('            let rendered_text = Arc::from(', '            let stage = std::time::Instant::now();\n            let rendered_text = Arc::from(');s=s.replace('            let semantic_selection = surface.selection_rows.is_some();', '            profile_text += stage.elapsed().as_nanos();\n            let stage = std::time::Instant::now();\n            let semantic_selection = surface.selection_rows.is_some();');s=s.replace('            measured_surfaces.push(TranscriptVisualEntry {', '            profile_selection += stage.elapsed().as_nanos();\n            measured_surfaces.push(TranscriptVisualEntry {');s=s.replace('    MeasuredTranscriptLayout {\n        sections: measured_sections,', '    eprintln!("PROFILE measure {width} {profile_surfaces} {profile_text} {profile_selection} {}", profile_start.elapsed().as_nanos());\n    MeasuredTranscriptLayout {\n        sections: measured_sections,');p.write_text(s)
 (out/'source.patch').write_bytes(subprocess.check_output(['git','diff'],cwd=root))
 with (out/'build.log').open('w') as log:
  metadata=json.loads(subprocess.check_output(['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--message-format','json'],cwd=root,stderr=log))
 (out/'nextest.json').write_text(json.dumps(metadata))
 binary=Path(next(iter(metadata['rust-suites'].values()))['binary-path']);shutil.copy2(binary,out/'profile.bin')
 (out/'receipt.json').write_text(json.dumps({'base':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'temporary instrumentation, single diagnostic stream run, excluded from acceptance results'},indent=2)+'\n')
 env={**os.environ,'HARNESS_REWRITE_SCENARIO':'stream','HARNESS_REWRITE_HISTORY':'1000','HARNESS_REWRITE_FRAMES':'200','HARNESS_REWRITE_PERF_OUT':str(out/'stream.json')}
 with (out/'stream.log').open('w') as log:subprocess.run(['cargo','nextest','run','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--profile','perf','-j1','--success-output','immediate'],cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 print('Stream preparation profile finished',flush=True)
finally:
 for f,b in original.items():(root/f).write_bytes(b)
print('Production source restored',flush=True)
