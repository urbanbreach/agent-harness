from pathlib import Path
out=Path(__file__).resolve().parent
head='''#![allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposerViewport { lines: Vec<String>, line_starts: Vec<usize>, cursor: Option<(usize,usize)> }
fn display_width(text: &str) -> usize { text.lines().map(unicode_width::UnicodeWidthStr::width).sum() }
#[derive(Clone, Copy)]
struct ComposerVisualChar<'a> { index: usize, text: &'a str, width: usize }
type ComposerVisualLines = (Vec<(String,usize)>,Option<(usize,usize)>);
mod legacy {
'''
tail='''
}
mod candidate { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui_composer/viewport.rs")); }
#[test]
fn recorded_composer_viewport_contract() {
 let pieces = ["a", " ", "\\t", "\\n", "\\r\\n", "界", "e\\u{301}", "👩‍💻", "\\u{301}", "\\u{200b}"];
 let mut texts = vec![String::new()];
 for a in pieces { for b in pieces { for c in pieces { texts.push(format!("{a}{b}{c}")); } } }
 texts.extend(["ab \\n", "ab \\nc", "ab\\r\\nc", " abc defghijk", "one\\n\\ntwo\\n", "  a  b  ", "🇫🇮🇬🇧👩‍👩‍👧‍👦"].map(str::to_owned));
 let mut cases=0;
 for text in texts { for width in [0,1,2,3,5,10] { for max_lines in [0,1,2,4] {
  for cursor in std::iter::once(None).chain((0..=text.chars().count()+1).map(Some)) {
   assert_eq!(candidate::composer_viewport(&text,width,max_lines,cursor),legacy::composer_viewport(&text,width,max_lines,cursor),"{text:?}, width {width}, rows {max_lines}, cursor {cursor:?}");
   cases+=1;
  }
 } } }
 eprintln!("{cases} exact predecessor/candidate viewport comparisons");
}
'''
source=head+(out/'legacy-viewport.rs').read_text()+tail
(out/'differential.rs').write_text(source)
Path('crates/harness-tui/tests/rewrite_viewport_probe.rs').write_text(source)
