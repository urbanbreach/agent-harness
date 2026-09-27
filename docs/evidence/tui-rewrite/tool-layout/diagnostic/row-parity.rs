use super::*;
#[path = "/tmp/tui-tool-layout/rows-before.rs"]
mod previous;

#[test]
fn diagnostic_ascii_selection_rows_match_previous() {
    let sources = ["", " ", "  x  ", "abcdefghi", "              ", "   before after   ",
        "界", "a界b", "界  b", "e\u{301}👩‍💻x", "#\u{fe0f} x", "\u{301}a", " \u{301} x",
        "a\t\r\nb", "a\u{200d}b", "👩\u{200d}💻", "👍🏽 x", "a\u{a0}b", "  α β  ",
        "!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
        "abcd\u{301}    end\u{301}   ", "abc\u{7f}def", " abc\u{1b} def", "\r\n\t", "12\u{301} 34\u{301}"];
    let mut comparisons = 0;
    for source in sources {
        let boundaries = source.char_indices().map(|(offset, _)| offset).chain([source.len()]);
        for boundary in boundaries {
            let line = Line::from(vec![Span::raw(&source[..boundary]), Span::raw(&source[boundary..])]);
            for width in (0..=16).chain([40, 155, 156]) {
                for rail in [None, Some(" "), Some("│"), Some(""), Some("界")] {
                    assert_eq!(
                        surface_selection_rows(std::slice::from_ref(&line), width, rail.is_some(), rail.unwrap_or(" ")),
                        previous::surface_selection_rows(std::slice::from_ref(&line), width, rail.is_some(), rail.unwrap_or(" ")),
                        "source={source:?} boundary={boundary} width={width} rail={rail:?}"
                    );
                    comparisons += 1;
                }
                for alignment in [Alignment::Left, Alignment::Center, Alignment::Right] {
                    assert_eq!(rows::aligned_selection_rows_for_line(&line, usize::from(width), alignment),
                        previous::aligned_selection_rows_for_line(&line, usize::from(width), alignment));
                    comparisons += 1;
                }
            }
        }
    }
    println!("matched {comparisons} selection row projections");
}
