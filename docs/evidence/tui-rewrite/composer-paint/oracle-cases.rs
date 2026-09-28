#[test]
fn recorded_composer_style_contract() {
    let pieces = ["a", " ", "\t", "\n", "\r\n", "界", "e\u{301}", "👩‍💻", "\u{301}", "\u{200b}"];
    let mut texts = vec![String::new(), "text ".repeat(100), "e\u{301}👩‍💻界".repeat(20), "@long/path/name\nnext".into()];
    for a in pieces { for b in pieces { for c in pieces { texts.push(format!("{a}{b}{c}")); } } }
    let mut cases = 0;
    for text in texts {
        for start in [0, 4] {
            let n = text.chars().count();
            for bounds in [vec![], vec![(start, start + 1)], vec![(start + 1, start + 2)], vec![(start + 2, start + 4)], vec![(start, start + n)], vec![(start + 1, start + 4), (start + 2, start + 5)]] {
                let tags: Vec<_> = bounds.into_iter().map(|(start, end)| app::FileMentionTag { start, end }).collect();
                for selection in [None, Some(start..start), Some(start + 1..start + 1), Some(start..start + n), Some(start + 1..start + 2), Some(start + 2..start + 4), Some(start..start + n + 2), Some(start + n + 1..start + n + 2)] {
                    for (base, tag) in [(Style::default().fg(Color::White).bg(Color::Black), Style::default().fg(Color::Yellow).bg(Color::Black).add_modifier(Modifier::BOLD)), (Style::default(), Style::default()), (Style::default().add_modifier(Modifier::REVERSED), Style::default().add_modifier(Modifier::BOLD).remove_modifier(Modifier::REVERSED))] {
                        assert_eq!(candidate::composer_line_with_file_tags(&text, start, &tags, base, tag, selection.clone()), legacy::composer_line_with_file_tags(&text, start, &tags, base, tag, selection.clone()), "text {text:?}, start {start}, selection {selection:?}");
                        cases += 1;
                    }
                }
            }
        }
    }
    eprintln!("{cases} exact complete styled-line comparisons");
}
