from pathlib import Path
import shutil
out=Path(__file__).resolve().parent
source=(out/'viewport-oracle.rs').read_text()
source=source.replace('mod candidate {', 'mod text { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/text.rs")); }\nmod candidate {')
Path('crates/harness-tui/tests/rewrite_viewport_probe.rs').write_text(source)
def height_part(s):return s[s.index('pub(crate) fn composer_input_height('):s.index('fn live_dock_rhythm(')]
head='''#![allow(dead_code)]
use harness_tui::composer_atoms::{AtomBuffer,AtomKind,WrappedLine,ComposerAtom,AttachmentId,FileMentionId,GraphemeCluster};
mod text { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/text.rs")); }
'''
constants='''use unicode_segmentation::UnicodeSegmentation;
const COMPOSER_VISIBLE_TEXT_CHROME: u16=6;
const MIN_COMPOSER_LINES: u16=1;
const MAX_COMPOSER_LINES: u16=6;
const PROMPT_MIN_MAX_HEIGHT: u16=6;
'''
s=(out/'legacy-buffer.rs').read_text();wrap=s[s.index('    pub fn wrap('):s.index('    fn parse_text(')];wrap=wrap.replace('pub fn wrap(&self, width:', 'fn legacy_wrap(buffer: &AtomBuffer, width:').replace('&self.atoms','buffer.atoms()')
source=head+wrap+'\nmod legacy_height {\n'+constants+height_part((out/'legacy-layout.rs').read_text())+'\n}\nmod candidate_height {\n'+constants+height_part(Path('crates/harness-tui/src/layout.rs').read_text())+'\n}\n'+(out/'oracle-cases.rs').read_text()
Path('crates/harness-tui/tests/rewrite_row_probe.rs').write_text(source)
