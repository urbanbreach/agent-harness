use std::collections::BTreeSet;

use super::error::{
    ActionError, CheckpointError, GeometryError, GeometrySubject, ScenarioError, TimingError,
};
use super::lifecycle::{validate_cleanup, validate_exit_code};
use super::metadata::{validate_adapters, validate_id, validate_schema_version};
use super::substitution::validate_substitutions;
use super::types::{
    CellPoint, Checkpoint, CheckpointName, KeyCode, Scenario, ScenarioAction, SemanticState, Tick,
    Viewport,
};

const REQUIRED_CHECKPOINTS: [CheckpointName; 3] = [
    CheckpointName::Rest,
    CheckpointName::Mid,
    CheckpointName::Settled,
];

pub(super) fn validate_scenario(scenario: &Scenario) -> Result<(), ScenarioError> {
    validate_schema_version(&scenario.schema_version)?;
    validate_id(&scenario.id.0)?;
    validate_adapters(&scenario.adapters)?;
    validate_viewport(&scenario.viewport, GeometrySubject::ScenarioViewport)?;
    let final_viewport = validate_actions(scenario)?;
    let last_action_tick = scenario
        .actions
        .last()
        .map(ScenarioAction::at_tick)
        .map(|tick| tick.0)
        .unwrap_or(0);
    validate_checkpoints(&scenario.checkpoints, final_viewport, last_action_tick)?;
    super::motion_validation::validate(scenario)?;
    validate_substitutions(&scenario.substitutions, &scenario.checkpoints)?;
    validate_exit_code(scenario.expected_exit.code)?;
    validate_cleanup(&scenario.cleanup)?;
    Ok(())
}

fn validate_actions(scenario: &Scenario) -> Result<Viewport, ScenarioError> {
    if scenario.actions.is_empty() {
        return Err(ScenarioError::NoActions);
    }
    let mut active_viewport = scenario.viewport;
    let mut previous_tick = None;
    for (index, action) in scenario.actions.iter().enumerate() {
        let tick = action.at_tick();
        validate_tick(tick, index, &mut previous_tick)?;
        active_viewport = validate_action(action, index, active_viewport)?;
    }
    Ok(active_viewport)
}

fn validate_tick(
    tick: Tick,
    index: usize,
    previous_tick: &mut Option<u64>,
) -> Result<(), ScenarioError> {
    if tick.0 == 0 {
        return Err(ScenarioError::InvalidTiming(TimingError::Zero { index }));
    }
    if let Some(previous) = *previous_tick {
        if tick.0 <= previous {
            return Err(ScenarioError::InvalidTiming(TimingError::OutOfOrder {
                index,
                previous,
                current: tick.0,
            }));
        }
    }
    *previous_tick = Some(tick.0);
    Ok(())
}

fn validate_action(
    action: &ScenarioAction,
    index: usize,
    active_viewport: Viewport,
) -> Result<Viewport, ScenarioError> {
    match action {
        ScenarioAction::TimedKey(action) => {
            if matches!(action.key.code, KeyCode::Char('\0')) {
                return Err(ScenarioError::InvalidAction(ActionError::NullCharacter));
            }
        }
        ScenarioAction::Paste(action) => {
            if action.text.is_empty() {
                return Err(ScenarioError::InvalidAction(ActionError::EmptyPaste));
            }
        }
        ScenarioAction::Mouse(action) => {
            validate_point(action.point, active_viewport, index)?;
        }
        ScenarioAction::Drag(action) => {
            validate_point(action.from, active_viewport, index)?;
            validate_point(action.to, active_viewport, index)?;
            if action.from == action.to {
                return Err(ScenarioError::InvalidGeometry(
                    GeometryError::DragHasNoDistance { action: index },
                ));
            }
        }
        ScenarioAction::Wheel(action) => {
            validate_point(action.point, active_viewport, index)?;
            if action.amount == 0 {
                return Err(ScenarioError::InvalidGeometry(
                    GeometryError::WheelAmountZero { action: index },
                ));
            }
        }
        ScenarioAction::Resize(action) => {
            validate_viewport(&action.viewport, GeometrySubject::Action(index))?;
            return Ok(action.viewport);
        }
        ScenarioAction::WaitForSemanticState(_) => {}
        ScenarioAction::TerminalReply(action) => {
            if action.response.is_empty() {
                return Err(ScenarioError::InvalidAction(
                    ActionError::EmptyTerminalReply,
                ));
            }
        }
    }
    Ok(active_viewport)
}

fn validate_point(
    point: CellPoint,
    viewport: Viewport,
    action: usize,
) -> Result<(), ScenarioError> {
    if point.col < viewport.cols && point.row < viewport.rows {
        Ok(())
    } else {
        Err(ScenarioError::InvalidGeometry(GeometryError::OutOfBounds {
            subject: GeometrySubject::Action(action),
        }))
    }
}

fn validate_viewport(viewport: &Viewport, subject: GeometrySubject) -> Result<(), ScenarioError> {
    if viewport.cols == 0 || viewport.rows == 0 {
        Err(ScenarioError::InvalidGeometry(
            GeometryError::EmptyViewport { subject },
        ))
    } else {
        Ok(())
    }
}

fn validate_checkpoints(
    checkpoints: &[Checkpoint],
    active_viewport: Viewport,
    last_action_tick: u64,
) -> Result<(), ScenarioError> {
    if checkpoints.len() != REQUIRED_CHECKPOINTS.len() {
        return Err(ScenarioError::InvalidCheckpoint(CheckpointError::Count {
            observed: checkpoints.len(),
        }));
    }
    let mut names = BTreeSet::new();
    let mut previous_tick = last_action_tick;
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        if !names.insert(checkpoint.name) {
            return Err(ScenarioError::InvalidCheckpoint(
                CheckpointError::Duplicate(checkpoint.name),
            ));
        }
        let expected = REQUIRED_CHECKPOINTS[index];
        if checkpoint.name != expected {
            return Err(ScenarioError::InvalidCheckpoint(
                CheckpointError::OutOfOrder {
                    expected,
                    observed: checkpoint.name,
                },
            ));
        }
        if checkpoint.at_tick.0 <= previous_tick {
            return Err(ScenarioError::InvalidTiming(
                TimingError::CheckpointBeforeActions {
                    checkpoint: checkpoint.name,
                },
            ));
        }
        previous_tick = checkpoint.at_tick.0;
        validate_viewport(
            &checkpoint.frame.viewport,
            GeometrySubject::Checkpoint(checkpoint.name),
        )?;
        if checkpoint.frame.viewport != active_viewport {
            return Err(ScenarioError::InvalidGeometry(
                GeometryError::ViewportMismatch {
                    subject: GeometrySubject::Checkpoint(checkpoint.name),
                },
            ));
        }
        if checkpoint.frame.capture_id.is_empty() {
            return Err(ScenarioError::InvalidCheckpoint(
                CheckpointError::EmptyCaptureId(checkpoint.name),
            ));
        }
        let expected_state = match checkpoint.name {
            CheckpointName::Rest => SemanticState::Rest,
            CheckpointName::Mid => SemanticState::Working,
            CheckpointName::Settled => SemanticState::Settled,
        };
        if checkpoint.frame.state != expected_state {
            return Err(ScenarioError::InvalidCheckpoint(
                CheckpointError::StateMismatch {
                    checkpoint: checkpoint.name,
                    expected: expected_state,
                    observed: checkpoint.frame.state,
                },
            ));
        }
    }
    Ok(())
}
