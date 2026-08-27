use crate::paths::chmod;
use crate::play_history::video_id;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub const MAX_ITEMS: usize = 80;
pub const REPEAT_MODES: [&str; 3] = ["off", "context", "track"];

pub fn load(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    let raw: Value = serde_json::from_str(&text).ok()?;
    if !raw.is_object() {
        return None;
    }
    Some(clip(&raw))
}

pub fn save(payload: &Value, path: &Path) -> Value {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
        chmod(parent, 0o700);
    }
    let session = clip(payload);
    if let Ok(text) = serde_json::to_string_pretty(&session) {
        let _ = fs::write(path, format!("{text}\n"));
        chmod(path, 0o600);
    }
    session
}

pub fn clip(payload: &Value) -> Value {
    let mut items: Vec<Value> = payload
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.is_object() && !video_id(row).is_empty())
        .collect();
    let mut index = payload.get("index").and_then(Value::as_i64).unwrap_or(0);
    if !items.is_empty() {
        index = index.clamp(0, items.len() as i64 - 1);
        if items.len() > MAX_ITEMS {
            let mut start = index.min((items.len() - MAX_ITEMS) as i64).max(0);
            start = start.min(index).max(0);
            let mut end = (start as usize + MAX_ITEMS).min(items.len());
            start = (end as i64 - MAX_ITEMS as i64).max(0);
            end = (start as usize + MAX_ITEMS).min(items.len());
            items = items[start as usize..end].to_vec();
            index -= start;
        }
    } else {
        index = -1;
    }
    let position_ms = payload
        .get("position_ms")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let mut repeat = payload
        .get("repeat")
        .and_then(Value::as_str)
        .unwrap_or("off")
        .to_string();
    if !REPEAT_MODES.contains(&repeat.as_str()) {
        repeat = "off".into();
    }
    json!({
        "items": items,
        "index": index,
        "shuffle": payload.get("shuffle").and_then(Value::as_bool).unwrap_or(false),
        "repeat": repeat,
        "position_ms": position_ms,
        "playing": payload.get("playing").and_then(Value::as_bool).unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_is_absent_not_empty() {
        let dir = tempdir().unwrap();
        assert!(load(&dir.path().join("play-queue.json")).is_none());
    }

    #[test]
    fn roundtrip_keeps_current_track() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("play-queue.json");
        let payload = json!({
            "items": [
                {"videoId": "aaa", "name": "One"},
                {"videoId": "bbb", "name": "Two"},
            ],
            "index": 1,
            "shuffle": true,
            "repeat": "context",
            "position_ms": 12345,
            "playing": true,
        });
        save(&payload, &path);
        let loaded = load(&path).unwrap();
        assert_eq!(loaded["index"], 1);
        assert_eq!(loaded["items"][1]["videoId"], "bbb");
        assert_eq!(loaded["shuffle"], true);
        assert_eq!(loaded["repeat"], "context");
        assert_eq!(loaded["position_ms"], 12345);
        assert_eq!(loaded["playing"], true);
    }

    #[test]
    fn clip_keeps_current_when_queue_is_long() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("play-queue.json");
        let items: Vec<Value> = (0..120)
            .map(|i| json!({"videoId": format!("v{i}"), "name": i.to_string()}))
            .collect();
        let saved = save(&json!({"items": items, "index": 90}), &path);
        let loaded = load(&path).unwrap();
        assert!(loaded["items"].as_array().unwrap().len() <= MAX_ITEMS);
        let idx = loaded["index"].as_i64().unwrap() as usize;
        assert_eq!(loaded["items"][idx]["videoId"], "v90");
        let saved_idx = saved["index"].as_i64().unwrap() as usize;
        assert_eq!(saved["items"][saved_idx]["videoId"], "v90");
    }

    #[test]
    fn corrupt_file_is_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("play-queue.json");
        fs::write(&path, "{not json").unwrap();
        assert!(load(&path).is_none());
    }
}
