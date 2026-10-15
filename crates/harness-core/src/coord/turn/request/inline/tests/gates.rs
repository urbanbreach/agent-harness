//! Candidate gates, skip-not-stop planning and the total cap backstop.
use super::*;
use crate::coord::context::archive::inline_cache::InlineCache;
use crate::coord::turn::request::inline::analysis::analyze;
use crate::coord::turn::request::inline::planner::{
    Item, Plan, Retraction, Selection, SystemCandidate, ToolCandidate,
};
use std::collections::BTreeSet;
use std::sync::Arc;

#[test]
fn only_the_final_normal_candidates_swap() {
    let counter = counter();
    let env = env(&counter, 90, ArchiveSystemPrompt::None, true);
    assert_eq!(env.shape.raw_frame_tokens(), 3136);
    // (id, text tokens, frames, error)
    let rows: [(&str, usize, usize, bool); 6] = [
        ("empty", 0, 0, false),
        ("floor", 2_999, 1, false),
        ("margin", 6_900, 2, false),
        ("failed", 10_000, 2, true),
        ("normal", 10_000, 2, false),
        ("newest", 10_000, 2, false),
    ];
    let sink = Arc::new(FakeSink::new(&rows.map(|(id, _, frames, _)| (id, frames))));
    let mut cache = with_sink(sink);
    let mut messages = vec![message(MessageRole::User, "go")];
    let mut flags = vec![Fact::default()];
    for (id, tokens, _, error) in rows {
        messages.push(call(id, "grep"));
        flags.push(Fact::default());
        messages.push(tool(
            id,
            &if tokens == 0 {
                String::new()
            } else {
                text(id, tokens)
            },
        ));
        flags.push(Fact {
            is_error: error,
            ..Fact::default()
        });
    }
    let analysis = analyze(
        &env,
        &messages,
        &Attachments::new(),
        &flags,
        &mut cache,
        false,
    );
    let swapped: Vec<_> = analysis
        .tools
        .built
        .iter()
        .filter(|tool| analysis.selection.tools.contains(&tool.index))
        .map(|tool| tool.id.as_str())
        .collect();
    assert_eq!(swapped, ["normal", "newest"]);
}

fn tools(frames: &[usize]) -> Vec<ToolCandidate> {
    frames
        .iter()
        .enumerate()
        .map(|(n, frames)| ToolCandidate {
            index: n + 1,
            frames: *frames,
        })
        .collect()
}

#[test]
fn an_oversize_candidate_is_skipped_and_selection_continues() {
    let candidates = tools(&[2, 2, 1, 2]);
    let plan = Plan {
        limit: 3,
        images_through: &[0; 6],
        system: None,
        tools: &candidates,
    };
    assert_eq!(plan.natural().tools, BTreeSet::from([1, 3]));
}

#[test]
fn the_backstop_retracts_the_newest_tool_and_never_a_real_image() {
    let candidates = [
        ToolCandidate {
            index: 3,
            frames: 2,
        },
        ToolCandidate {
            index: 5,
            frames: 2,
        },
    ];
    // system 2 frames on the user at 1; a screenshot arrives at 7
    let images = [0, 0, 0, 0, 0, 0, 0, 1];
    let plan = Plan {
        limit: 6,
        images_through: &images,
        system: Some(SystemCandidate {
            carrier: 1,
            frames: 2,
        }),
        tools: &candidates,
    };
    let natural = plan.natural();
    assert_eq!(
        natural,
        Selection {
            system: true,
            tools: BTreeSet::from([3, 5])
        }
    );
    let (selection, retractions) = plan.replay(&natural);
    assert_eq!(
        selection,
        Selection {
            system: true,
            tools: BTreeSet::from([3])
        }
    );
    assert_eq!(
        retractions,
        [Retraction {
            item: Item::Tool(5),
            at: 7
        }]
    );
    // Source images alone over the cap leave nothing but real images.
    let crowded = Plan {
        limit: 1,
        images_through: &[0, 0, 0, 0, 0, 0, 0, 3],
        ..plan
    };
    let (selection, _) = crowded.replay(&crowded.natural());
    assert_eq!(selection, Selection::default());
}
