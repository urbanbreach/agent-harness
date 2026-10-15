use super::*;
use harness_providers::{CompletionMessage, MessageRole};

#[test]
fn rollback_restores_each_touched_entry_once() {
    let mut context = Context::default();
    context.push(CompletionMessage::text(MessageRole::User, "one"), 1, None);
    context.push(CompletionMessage::text(MessageRole::User, "two"), 2, None);
    let mut rewrite = Rewrite::default();
    rewrite.remember(&context, 0);
    context.entries[0].message.content = "changed".into();
    // A second edit to the same entry must not overwrite the remembered original.
    rewrite.remember(&context, 0);
    rewrite.remember(&context, 1);
    context.entries[1].message.content = "changed".into();
    rewrite.restore(&mut context);
    assert_eq!(context.entries[0].message.content, "one");
    assert_eq!(context.entries[1].message.content, "two");
}

#[test]
fn boundaries_follow_the_latest_and_first_summary() {
    let messages: Vec<_> = (0..4)
        .map(|_| CompletionMessage::text(MessageRole::User, "x"))
        .collect();
    for (summaries, prune, shake) in [
        (vec![], 0, 0),
        (vec![0], 1, 1),
        (vec![1], 2, 2),
        (vec![0, 2], 3, 1),
    ] {
        let items: Vec<_> = messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                let mut item = Item::new(message);
                if summaries.contains(&index) {
                    item.kind = ItemKind::Summary;
                }
                item
            })
            .collect();
        assert_eq!(prune_boundary(&items), prune, "{summaries:?}");
        assert_eq!(shake_boundary(&items), shake, "{summaries:?}");
    }
}
