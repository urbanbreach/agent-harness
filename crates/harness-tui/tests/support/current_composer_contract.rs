//! Reviewed divergences from the frozen rewrite oracle, never a recorder.
//! ed86901e made the build version 0.0.1; version cells now expect build metadata.
//! 9b4028dc made Enter steer and Alt+i queue. Only the 37 footer labels and four
//! queue gestures in the identity-bound patch fixture change; all cells, styles,
//! cursor positions, inputs and coordinator intents still compare exactly.
use super::*;

const FIXTURE: &str = "tests/fixtures/tui-current-composer-20261008.patches.jsonl";
const IDENTITY: &str = "tests/fixtures/tui-current-composer-20261008.identity.json";

pub(super) struct Contract {
    patches: Vec<Value>,
}

impl Contract {
    pub(super) fn load() -> Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bytes = fs::read(root.join(FIXTURE))?;
        let identity: Value = serde_json::from_slice(&fs::read(root.join(IDENTITY))?)?;
        let binding = json!({
            "contract": "Enter steers; Alt+i queues (9b4028dc, 2026-10-08)",
            "original_base": BASE,
            "original_fixture_sha256": current_settings::sha256(&fs::read(root.join(GOLDEN))?),
            "footer": ["Enter:queue", "Enter:steer"],
            "queue_input": [{"key": "Enter", "modifiers": 0}, {"key": "Char('i')", "modifiers": 4}],
        });
        if identity["binding"] != binding
            || identity["fixture_sha256"] != current_settings::sha256(&bytes)
        {
            return Err("current composer contract identity differs".into());
        }
        let patches: Vec<Value> = std::str::from_utf8(&bytes)?
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(patches.len(), 38, "composer patch matrix is incomplete");
        assert_eq!(
            patches
                .iter()
                .filter(|patch| patch["queued"] == true)
                .count(),
            4
        );
        assert_eq!(
            patches
                .iter()
                .filter(|patch| !patch["footer"].is_null())
                .count(),
            37
        );
        Ok(Self { patches })
    }

    pub(super) fn apply(&self, expected: &mut Value) -> Result {
        let patch = self
            .patches
            .iter()
            .find(|patch| patch["id"] == expected["id"]);
        if patch.is_none() && !contains_version_token(expected, "0.1.0")? {
            return Ok(());
        }
        let mut cells = expand_cells(expected)?;
        if let Some(patch) = patch {
            if let Some(start) = patch["footer"].as_u64() {
                let start = usize::try_from(start)?;
                let label = cells[start..start + 11]
                    .iter()
                    .map(|cell| cell[0].as_str().unwrap_or_default())
                    .collect::<String>();
                assert_eq!(label, "Enter:queue", "composer patch preimage differs");
                for (offset, ch) in "steer".chars().enumerate() {
                    cells[start + 6 + offset][0] = json!(ch.to_string());
                }
            }
            if patch["queued"] == true {
                let input = expected["inputs"]
                    .as_array_mut()
                    .ok_or("missing queue inputs")?
                    .last_mut()
                    .ok_or("missing queue gesture")?;
                assert_eq!(*input, json!({"key": "Enter", "modifiers": 0}));
                *input = json!({"key": "Char('i')", "modifiers": 4});
            }
        }
        expect_build_version(&mut cells, "0.1.0", env!("CARGO_PKG_VERSION"))?;
        expected["cells"] = json!(cell_runs(cells.into_iter()));
        Ok(())
    }
}

// Most frames need no patch. Scan borrowed run symbols before cloning all cells.
fn contains_version_token(frame: &Value, source: &str) -> Result<bool> {
    let mut text = String::new();
    for run in frame["cells"].as_array().ok_or("missing reference cells")? {
        let count = usize::try_from(run[0].as_u64().ok_or("invalid cell run")?)?;
        let symbol = run[1][0].as_str().ok_or("missing cell symbol")?;
        for _ in 0..count {
            text.push_str(symbol);
        }
    }
    Ok(text.contains(source))
}

fn expand_cells(frame: &Value) -> Result<Vec<Value>> {
    let mut cells = Vec::new();
    for run in frame["cells"].as_array().ok_or("missing reference cells")? {
        cells.extend(std::iter::repeat_n(
            run[1].clone(),
            usize::try_from(run[0].as_u64().ok_or("invalid cell run")?)?,
        ));
    }
    Ok(cells)
}

pub(super) fn build_version(frame: &mut Value, source: &str, target: &str) -> Result {
    if !contains_version_token(frame, source)? {
        return Ok(());
    }
    let mut cells = expand_cells(frame)?;
    expect_build_version(&mut cells, source, target)?;
    frame["cells"] = json!(cell_runs(cells.into_iter()));
    Ok(())
}

fn expect_build_version(cells: &mut [Value], source: &str, target: &str) -> Result {
    let length = source.chars().count();
    // Replace only the known version token after Harness or before the current
    // release heading. This predicts the exact build value, not a wildcard or
    // normalization of candidate output; incorrect version paint still fails.
    let symbols = cells
        .iter()
        .map(|cell| cell[0].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    let starts = symbols
        .windows(length)
        .enumerate()
        .filter_map(|(start, token)| {
            (token
                .iter()
                .flat_map(|symbol| symbol.chars())
                .eq(source.chars())
                && (start >= 8
                    && matches!(
                        symbols[start - 8..start].concat().as_str(),
                        "Harness " | "HARNESS "
                    )
                    || symbols
                        .get(start + length..start + length + 10)
                        .is_some_and(|suffix| suffix.concat() == " — current")))
            .then_some(start)
        })
        .collect::<Vec<_>>();
    for start in starts {
        let release_heading = cells.get(start + length).is_some_and(|cell| cell[0] == " ")
            && cells
                .get(start + length + 1)
                .is_some_and(|cell| cell[0] == "—");
        let old_length = if release_heading { length + 10 } else { length };
        let text = if release_heading {
            format!("{target} — current")
        } else if start >= 8 && cells[start - 7][0] == "A" {
            // The recorded replay sidebar caps its build label at twelve cells.
            target.chars().take(12).collect()
        } else {
            target.to_string()
        };
        let template = cells[start].clone();
        let mut characters = text.chars();
        for offset in 0..old_length.max(text.chars().count()) {
            let cell = cells
                .get_mut(start + offset)
                .ok_or("version exceeds reference frame")?;
            *cell = template.clone();
            cell[0] = json!(characters.next().unwrap_or(' ').to_string());
        }
    }
    Ok(())
}
