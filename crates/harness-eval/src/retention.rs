use crate::{cell::Cell, session::Inner};
use serde_json::{json, Value};
use std::{collections::VecDeque, path::PathBuf, sync::atomic::Ordering};

struct Entry {
    id: String,
    bytes: usize,
    images: Vec<(PathBuf, usize)>,
}

#[derive(Default)]
pub(crate) struct Retained {
    entries: VecDeque<Entry>,
    bytes: usize,
    image_bytes: usize,
}

impl Retained {
    fn remove_images(&mut self, entry: &Entry) {
        for (path, bytes) in &entry.images {
            let _ = std::fs::remove_file(path);
            self.image_bytes = self.image_bytes.saturating_sub(*bytes);
        }
    }
}

impl Drop for Retained {
    fn drop(&mut self) {
        while let Some(entry) = self.entries.pop_front() {
            self.remove_images(&entry);
        }
    }
}

impl Inner {
    pub async fn retain(&self, id: &str) {
        let mut retained = self.completed.lock().await;
        let mut cells = self.cells.lock().await;
        let Some(cell) = cells.get(id) else {
            return;
        };
        let mut state = cell.state.lock().await;
        let Some(result) = state.result.as_mut() else {
            return;
        };
        let mut entry = Entry {
            id: id.into(),
            bytes: 0,
            images: Vec::new(),
        };
        for part in result["content"].as_array_mut().into_iter().flatten() {
            if part["type"] != "image" {
                continue;
            }
            let Some(data) = part["data"].as_str() else {
                continue;
            };
            let path = self.options.artifacts.join(format!(
                "image-{}.b64",
                self.sequence.fetch_add(1, Ordering::Relaxed)
            ));
            if std::fs::write(&path, data).is_ok() {
                let length = data.len();
                entry.images.push((path.clone(), length));
                retained.image_bytes += length;
                *part = json!({"type":"spilled-image","path":path,"length":length,"mimeType":part["mimeType"]});
            }
        }
        entry.bytes = result.to_string().len();
        retained.bytes += entry.bytes;
        retained.entries.push_back(entry);
        state.images.clear();
        state.json.clear();
        state.statuses.clear();
        state.tool_calls = crate::metadata::ToolCalls::default();
        state.output.release();
        drop(state);
        let budget = self.options.settings.memory.retained_results_mb * 1024 * 1024;
        while retained.entries.len() > 1
            && (retained.entries.len() > 32 || budget > 0 && retained.bytes > budget)
        {
            if let Some(entry) = retained.entries.pop_front() {
                cells.remove(&entry.id);
                retained.bytes = retained.bytes.saturating_sub(entry.bytes);
                retained.remove_images(&entry);
            }
        }
        let budget = self.options.settings.memory.retained_images_mb * 1024 * 1024;
        let mut remaining = retained.image_bytes;
        if budget == 0 {
            return;
        }
        for (path, bytes) in retained
            .entries
            .iter_mut()
            .filter(|entry| entry.id != id)
            .flat_map(|entry| &mut entry.images)
        {
            if remaining <= budget {
                break;
            }
            let _ = std::fs::remove_file(path);
            remaining = remaining.saturating_sub(*bytes);
            *bytes = 0;
        }
        retained.image_bytes = remaining;
    }
}

impl Cell {
    pub(crate) fn hydrate(mut result: Value) -> Value {
        for part in result["content"].as_array_mut().into_iter().flatten() {
            if part["type"] != "spilled-image" {
                continue;
            }
            let path = part["path"].as_str().unwrap_or_default();
            *part = match std::fs::read_to_string(path) {
                Ok(data) => json!({"type":"image","mimeType":part["mimeType"],"data":data}),
                Err(error) => {
                    json!({"type":"text","text":format!("[{} image ({} base64 bytes) of this settled cell is no longer available: {path} ({error})]",part["mimeType"].as_str().unwrap_or("image"),part["length"])})
                }
            };
        }
        result
    }
}
