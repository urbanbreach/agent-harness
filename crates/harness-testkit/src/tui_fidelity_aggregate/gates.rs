use std::path::Path;

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::Deserialize;

use super::AggregateError;
use crate::tui_fidelity_compare::{AcceptanceProfile, ComparisonReceipt};

pub(super) fn valid(comparison: &ComparisonReceipt, profile: AcceptanceProfile) -> bool {
    const ALL: [&str; 9] = [
        "presentation",
        "semantic_cell",
        "pixel",
        "motion",
        "timing",
        "provenance",
        "checkpoint",
        "exit",
        "cleanup",
    ];
    const PACKET2_REQUIRED: [&str; 5] = [
        "presentation",
        "provenance",
        "checkpoint",
        "exit",
        "cleanup",
    ];
    comparison.gates.len() == ALL.len()
        && ALL.iter().all(|name| comparison.gates.contains_key(*name))
        && match profile {
            AcceptanceProfile::FullParity => ALL
                .iter()
                .all(|name| comparison.gates.get(*name).is_some_and(|gate| gate.passed)),
            AcceptanceProfile::Packet2Scheduling => PACKET2_REQUIRED
                .iter()
                .all(|name| comparison.gates.get(*name).is_some_and(|gate| gate.passed)),
        }
}

#[derive(Deserialize)]
struct GateEnvelope {
    #[serde(deserialize_with = "unique_gate_map")]
    gates: (),
}

pub(super) fn reject_duplicates(path: &Path) -> Result<(), AggregateError> {
    let bytes = std::fs::read(path).map_err(|error| AggregateError::Evidence {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let _: GateEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| AggregateError::Evidence {
            path: path.to_path_buf(),
            detail: format!("invalid or duplicate comparison gate: {error}"),
        })?;
    Ok(())
}

fn unique_gate_map<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct UniqueGateVisitor;

    impl<'de> Visitor<'de> for UniqueGateVisitor {
        type Value = ();

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a comparison gate map with unique names")
        }

        fn visit_map<M>(self, mut map: M) -> Result<(), M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut names = std::collections::BTreeSet::new();
            while let Some(name) = map.next_key::<String>()? {
                if !names.insert(name.clone()) {
                    return Err(serde::de::Error::custom(format!("duplicate gate `{name}`")));
                }
                map.next_value::<IgnoredAny>()?;
            }
            Ok(())
        }
    }

    deserializer.deserialize_map(UniqueGateVisitor)
}
