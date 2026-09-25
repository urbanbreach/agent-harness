#!/usr/bin/env python3
"""Regenerate cell hashes directly from the checked-out Grok rewind renderer."""
from pathlib import Path
import tempfile, subprocess, json, hashlib
root=Path(__file__).resolve().parents[1]
src=root/'inspirations/grok-build/crates/codegen'
work=Path(tempfile.mkdtemp(prefix='grok-rewind-reference-'))
(work/'src/views').mkdir(parents=True)
(work/'src/render').mkdir()
(work/'src/theme').mkdir()
(work/'Cargo.toml').write_text('''[package]
name="grok-rewind-reference"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
ratatui={version="0.30.1",features=["crossterm_0_29"]}
crossterm="0.29"
serde={version="1",features=["derive"]}
serde_json="1"
unicode-width="0.2"
sha2="0.11"
''')
source_hashes={}
for relative in ['xai-grok-pager-render/src/theme/tokyonight.rs', 'xai-grok-pager-render/src/theme/groknight.rs', 'xai-grok-pager-render/src/theme/grokday.rs', 'xai-grok-pager-render/src/theme/terminal_default.rs', 'xai-grok-pager-render/src/render/color.rs', 'xai-grok-pager-render/src/util.rs']:
 source=src/relative
 source_hashes[str(source.relative_to(root))]=hashlib.sha256(source.read_bytes()).hexdigest()
for filename in ['rewind.rs','overlay_list.rs']:
 source=src/'xai-grok-pager/src/views'/filename
 source_hashes[str(source.relative_to(root))]=hashlib.sha256(source.read_bytes()).hexdigest()
 (work/'src/views'/filename).write_text(source.read_text().split('#[cfg(test)]')[0])
# Use the reference palette and rendering helpers themselves. Only app-global theme selection and prompt storage are stubbed.
theme=(src/'xai-grok-pager-render/src/theme/tokyonight.rs').read_text().split('impl Theme {')[0]
(work/'src/theme/tokyonight.rs').write_text(theme)
for name in ['groknight.rs','grokday.rs','terminal_default.rs']:
 (work/'src/theme'/name).write_text((src/'xai-grok-pager-render/src/theme'/name).read_text().split('#[cfg(test)]')[0])
(work/'src/render/color.rs').write_text((src/'xai-grok-pager-render/src/render/color.rs').read_text().split('#[cfg(test)]')[0])
util=(src/'xai-grok-pager-render/src/util.rs').read_text()
trunc=util[util.index('pub fn truncate_to_width('):util.index('/// Left-align')]
(work/'src/util.rs').write_text('use std::borrow::Cow; use unicode_width::UnicodeWidthChar;\n'+trunc)
cases=[]
for w,h in [(9,6),(20,12),(40,24),(80,24),(120,40)]:
 for focused in [True,False]:
  for phase,indices in [('loading',[0]),('picker',[0,14,19]),('cancel',[0,1]),('confirm',[0,1,2]),('executing',[0]),('error',[0])]:
   for selected in indices: cases.append(dict(width=w,screen_height=h,focused=focused,phase=phase,selected=selected))
cases=[dict(case,theme=theme) for theme in ['dark','light','terminal'] for case in cases]
(work/'cases.json').write_text(json.dumps(cases))
main=r'''
#![allow(dead_code,unused_imports)]
mod theme {
 pub mod tokyonight; mod groknight; mod grokday; mod terminal_default; pub use tokyonight::Theme;
 impl Theme {
  pub fn current()->Self { match crate::THEME.load(std::sync::atomic::Ordering::Relaxed) {1=>Self::grokday(),2=>Self::terminal_default(),_=>Self::groknight()} }
  pub fn selection_overlay(&self)->ratatui::style::Style { if self.bg_base==ratatui::style::Color::Reset {ratatui::style::Style::new().add_modifier(ratatui::style::Modifier::REVERSED)} else {ratatui::style::Style::new().bg(self.bg_visual)} }
 }
}
mod util;
mod render { pub mod color; pub mod line_utils { pub fn truncate_str(s:&str,w:usize)->String { crate::util::truncate_to_width(s,w).into_owned() } } }
mod glyphs { pub fn accent_bar()->&'static str { "┃" } pub fn filled_dot()->&'static str { "●" } }
mod views { pub mod rewind; pub mod overlay_list; pub mod prompt_widget { #[derive(Debug)] pub struct StashedPrompt; } }
use views::rewind::*;
use ratatui::{buffer::Buffer,layout::Rect,style::Style};
use sha2::{Sha256,Digest};
static THEME:std::sync::atomic::AtomicU8=std::sync::atomic::AtomicU8::new(0);
fn main() {
 let mut cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../cases.json")).unwrap();
 for c in &mut cases {
  THEME.store(match c["theme"].as_str().unwrap() {"light"=>1,"terminal"=>2,_=>0},std::sync::atomic::Ordering::Relaxed);
  let selected=c["selected"].as_u64().unwrap() as usize;
  let phase=match c["phase"].as_str().unwrap() {
   "loading"=>RewindPhase::Loading,
   "picker"=>RewindPhase::Picker { selected, points:(0..20).rev().map(|i|RewindPointInfo {prompt_index:i,created_at:String::new(),num_file_snapshots:0,has_file_changes:false,prompt_preview:Some(format!("Turn {}: inspect 日本語 and a long prompt with spaces",i+1))}).collect() },
   "cancel"=>RewindPhase::CancelOffer {active_idx:selected},
   "confirm"=>RewindPhase::Confirm {target_prompt_index:3,active_idx:selected,prompt_preview:Some("inspect 日本語 and a long prompt with spaces".into())},
   "executing"=>RewindPhase::Executing {target_prompt_index:3},
   _=>RewindPhase::Error {message:"The session could not be rewound. Try again.".into()},
  };
  let height=rewind_overlay_height(&phase,c["screen_height"].as_u64().unwrap() as u16);
  let area=Rect::new(2,1,c["width"].as_u64().unwrap() as u16,height);
  let mut buffer=Buffer::empty(Rect::new(0,0,area.right()+2,area.bottom()+1));
  buffer.set_style(buffer.area,Style::new().fg(theme::Theme::current().text_primary).bg(theme::Theme::current().bg_base));
  render_rewind_overlay(&mut buffer,area,&phase,c["focused"].as_bool().unwrap());
  let cells:Vec<_>=buffer.content.iter().map(|cell|(cell.symbol(),format!("{:?}",cell.fg),format!("{:?}",cell.bg),cell.modifier.bits())).collect();
  c["height"]=height.into();
  c["digest"]=Sha256::digest(serde_json::to_vec(&cells).unwrap()).iter().map(|b|format!("{b:02x}")).collect::<String>().into();
  if c["width"]==80 && c["focused"]==true { c["cells"]=serde_json::to_value(&cells).unwrap(); }
 }
 println!("{}",serde_json::to_string_pretty(&cases).unwrap());
}
'''
(work/'src/main.rs').write_text(main)
print(work,flush=True)
import os
env=dict(os.environ, CARGO_TARGET_DIR=str(root/'target/grok-rewind-reference'))
result=subprocess.run(['cargo','run','--offline','--quiet','--manifest-path',str(work/'Cargo.toml')],capture_output=True,text=True,env=env)
if result.returncode: print(result.stderr);raise SystemExit(result.returncode)
data=json.loads(result.stdout)
(root/'crates/harness-tui/tests/fixtures/grok-rewind-reference.json').write_text(json.dumps({'source_sha256':source_hashes,'cases':[{k:v for k,v in c.items() if k!='cells'} for c in data]},indent=2)+'\n')
Path('/tmp/grok-rewind-reference-cells.json').write_text(json.dumps(data))

import shutil
shutil.rmtree(work)
