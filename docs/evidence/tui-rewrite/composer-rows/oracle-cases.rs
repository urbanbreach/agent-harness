#[test]
fn recorded_atom_wrap_contract() {
    let mut cases=0;
    for a in 0..7 { for b in 0..7 { for c in 0..7 {
        let atoms=[a,b,c].into_iter().enumerate().map(|(index,kind)| {
            let id=index as u64;
            match kind {
                0=>ComposerAtom::text(id,GraphemeCluster::new("x")),
                1=>ComposerAtom::text(id,GraphemeCluster::new("界")),
                2=>ComposerAtom::attachment(id,AttachmentId::new(1)),
                3=>ComposerAtom::file_mention(id,FileMentionId::new(1)),
                4=>ComposerAtom::newline(id),
                5=>{let mut atom=ComposerAtom::text(id,GraphemeCluster::new("x"));atom.display_width=u16::MAX;atom},
                _=>{let mut atom=ComposerAtom::newline(id);atom.display_width=5;atom},
            }
        }).collect();
        let buffer=AtomBuffer::from_atoms(atoms).expect("unique ids");
        for width in [0,1,2,3,10,u16::MAX-1,u16::MAX] {
            assert_eq!(buffer.wrap(width),legacy_wrap(&buffer,width),"kinds {a}/{b}/{c}, width {width}");cases+=1;
        }
    } } }
    for text in ["","\n","\n\n","a\n", "e\u{301} 👩‍💻"] {
        let buffer=AtomBuffer::from_text(text);
        for width in [0,1,2,3,10,u16::MAX] { assert_eq!(buffer.wrap(width),legacy_wrap(&buffer,width));cases+=1; }
    }
    eprintln!("{cases} exact atom-row comparisons");
}
#[test]
fn recorded_row_height_contract() {
    let pieces=["a"," ","\t","\n","\r\n","界","e\u{301}","👩‍💻","\u{301}","\u{200b}"];
    let mut texts=vec![String::new(), "line\n".repeat(100), "x".repeat(1000)];
    for a in pieces {for b in pieces {for c in pieces { texts.push(format!("{a}{b}{c}")); }}}
    let mut cases=0;
    for text in texts {for width in [0,1,6,7,8,10,16,40,120] {
        assert_eq!(candidate_height::composer_input_height(&text,width),legacy_height::composer_input_height(&text,width),"live {text:?}, width {width}");cases+=1;
        for height in [0,18,48,u16::MAX] {
            assert_eq!(candidate_height::startup_composer_input_height(&text,width,height),legacy_height::startup_composer_input_height(&text,width,height),"startup {text:?}, width {width}, height {height}");cases+=1;
        }
    }}
    eprintln!("{cases} exact frame-height comparisons");
}
