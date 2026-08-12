mod error;
mod error_display;
mod lifecycle;
mod metadata;
mod motion_validation;
mod names;
mod substitution;
mod types;
mod validation;

pub use error::{
    ActionError, CheckpointError, CleanupError, ExitCodeError, GeometryError, GeometrySubject,
    MotionCaptureError, ScenarioError, SubstitutionError, TimingError,
};
pub use types::*;

pub const SCENARIO_SCHEMA_VERSION: &str = "tui-fidelity-scenario-v1";

impl Scenario {
    pub fn from_json(input: &str) -> Result<Self, ScenarioError> {
        let scenario: Self = serde_json::from_str(input)
            .map_err(|error| ScenarioError::Deserialize(error.to_string()))?;
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn to_json(&self) -> Result<String, ScenarioError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| ScenarioError::Serialize(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), ScenarioError> {
        validation::validate_scenario(self)
    }

    pub fn validate_for_adapter(&self, adapter: AdapterKind) -> Result<(), ScenarioError> {
        self.validate()?;
        metadata::validate_adapter_selection(&self.adapters, adapter)
    }
}
