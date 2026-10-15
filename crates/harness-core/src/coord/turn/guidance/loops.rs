//! Repeated tool calls inside one turn: a reminder first, then a stop.
use super::*;

/// Repetition units are at most three calls long, and a turn stops at twice the threshold.
const MAX_LOOP_PERIOD: usize = 3;

impl Worker {
    pub(super) async fn loop_guard(
        &mut self,
        calls: &[AssistantToolCall],
        messages: &mut Context,
    ) -> Result<(), CoordinatorError> {
        let settings = &self.behavior.loop_guard;
        if !settings.enabled || settings.threshold < 2 {
            return Ok(());
        }
        // Room for the stop count of the longest cycle; config validation bounds the threshold.
        let window = MAX_LOOP_PERIOD * 2 * settings.threshold as usize;
        for call in calls {
            if settings
                .exempt_tools
                .iter()
                .any(|tool| tool == &call.function_name)
            {
                // Waits and questions can change what the next identical call returns.
                self.guidance.calls.clear();
                continue;
            }
            if self.guidance.calls.len() >= window {
                self.guidance.calls.pop_front();
            }
            self.guidance.calls.push_back(Call::of(call));
        }
        let Some(repeat) = repetition(&self.guidance.calls) else {
            return Ok(());
        };
        let threshold = settings.threshold as usize;
        if repeat.count >= threshold * 2 {
            return Err(CoordinatorError::Invalid(format!(
                "loop guard stopped the turn: {} repeated {} times after a reminder",
                repeat.describe(),
                repeat.count
            )));
        }
        if repeat.count < threshold || self.guidance.loop_reminded.as_ref() == Some(&repeat.unit) {
            return Ok(());
        }
        self.guidance.loop_reminded = Some(repeat.unit.clone());
        let remaining = threshold * 2 - repeat.count;
        let body = format!(
            "You have made {} {} times in a row. Repeating it will not produce a different result. Use what the earlier results already show, try a materially different approach, or stop and report what is blocking you. If it repeats {remaining} more times, Harness will stop this turn.",
            repeat.describe(),
            repeat.count
        );
        self.remind(RuntimeReminderKind::LoopGuard, body, None, messages)
            .await
    }
}

/// Tool name plus a digest of its canonical arguments, so long arguments are not retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct Call {
    name: String,
    arguments: u64,
}

impl Call {
    fn of(call: &AssistantToolCall) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        match serde_json::from_str::<Value>(&call.arguments_json) {
            Ok(value) => canonical_json(value).hash(&mut hasher),
            Err(_) => call.arguments_json.hash(&mut hasher),
        }
        Self {
            name: call.function_name.clone(),
            arguments: hasher.finish(),
        }
    }
}

fn canonical_json(mut value: Value) -> String {
    value.sort_all_objects();
    value.to_string()
}

struct Repetition {
    /// Calls in the repeated unit, oldest first.
    unit: Vec<Call>,
    count: usize,
}

impl Repetition {
    fn describe(&self) -> String {
        match self.unit.as_slice() {
            [call] => format!("the same `{}` call with identical arguments", call.name),
            _ => format!(
                "the same sequence of {} calls ({})",
                self.unit.len(),
                self.unit
                    .iter()
                    .map(|call| format!("`{}`", call.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The longest trailing run of an identical call, or of a repeated cycle of two or three
/// distinct calls, counted in repetitions of the unit.
fn repetition(calls: &VecDeque<Call>) -> Option<Repetition> {
    let calls: Vec<&Call> = calls.iter().collect();
    let mut best: Option<Repetition> = None;
    for period in 1..=MAX_LOOP_PERIOD {
        if calls.len() < period * 2 {
            continue;
        }
        let unit = &calls[calls.len() - period..];
        if period > 1 && unit.iter().all(|call| *call == unit[0]) {
            continue;
        }
        let mut count = 1;
        while calls.len() >= period * (count + 1)
            && calls[calls.len() - period * (count + 1)..calls.len() - period * count] == *unit
        {
            count += 1;
        }
        if count >= 2 && best.as_ref().is_none_or(|b| count > b.count) {
            best = Some(Repetition {
                unit: unit.iter().map(|call| (*call).clone()).collect(),
                count,
            });
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls(names: &[&str]) -> VecDeque<Call> {
        names
            .iter()
            .map(|name| Call {
                name: (*name).into(),
                arguments: 0,
            })
            .collect()
    }

    #[test]
    fn repetition_counts_identical_calls_and_cycles_but_not_progress() {
        for (sequence, expected) in [
            (&["read", "read", "read"][..], Some((1, 3))),
            (&["grep", "read", "read", "read", "read"][..], Some((1, 4))),
            (
                &["read", "edit", "read", "edit", "read", "edit"][..],
                Some((2, 3)),
            ),
            (&["a", "b", "c", "a", "b", "c"][..], Some((3, 2))),
            (&["read", "edit", "bash", "read"][..], None),
            (&["read"][..], None),
        ] {
            let found = repetition(&calls(sequence)).map(|r| (r.unit.len(), r.count));
            assert_eq!(found, expected, "{sequence:?}");
        }
    }
}
