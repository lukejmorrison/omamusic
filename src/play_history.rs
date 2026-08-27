use crate::catalog::watch_url;
use crate::json_util::get_text;
use crate::paths::chmod;
use serde_json::Value;
use std::fs;
use std::path::Path;

pub const MAX_ITEMS: usize = 80;

pub fn video_id(item: &Value) -> String {
    let vid = get_text(item, "videoId");
    if !vid.is_empty() {
        vid
    } else {
        get_text(item, "id")
    }
}

pub fn load(path: &Path) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(path) else {
        return vec![];
    };
    let Ok(Value::Array(raw)) = serde_json::from_str::<Value>(&text) else {
        return vec![];
    };
    raw.into_iter()
        .filter(|row| row.is_object() && !video_id(row).is_empty())
        .take(MAX_ITEMS)
        .collect()
}

pub fn save(rows: &[Value], path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
        chmod(parent, 0o700);
    }
    let payload: Vec<&Value> = rows
        .iter()
        .filter(|row| row.is_object() && !video_id(row).is_empty())
        .take(MAX_ITEMS)
        .collect();
    if let Ok(text) = serde_json::to_string_pretty(&payload) {
        let _ = fs::write(path, format!("{text}\n"));
        chmod(path, 0o600);
    }
}

pub fn remember(item: &Value, existing: &[Value], path: &Path) -> Vec<Value> {
    let mut track = item.clone();
    let vid = video_id(&track);
    if vid.is_empty() {
        return existing.to_vec();
    }
    let mut rows: Vec<Value> = existing
        .iter()
        .filter(|row| video_id(row) != vid)
        .cloned()
        .collect();
    let url = get_text(&track, "externalUrl");
    if let Some(obj) = track.as_object_mut() {
        obj.insert("videoId".into(), Value::String(vid.clone()));
        if url.is_empty() {
            obj.insert("externalUrl".into(), Value::String(watch_url(&vid)));
        }
    }
    rows.insert(0, track);
    rows.truncate(MAX_ITEMS);
    save(&rows, path);
    rows
}

pub fn merge(local: &[Value], remote: &[Value]) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in local.iter().chain(remote.iter()) {
        if !row.is_object() {
            continue;
        }
        let vid = video_id(row);
        if vid.is_empty() || !seen.insert(vid) {
            continue;
        }
        out.push(row.clone());
        if out.len() >= MAX_ITEMS {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn remember_puts_latest_first_and_dedupes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("play-history.json");
        let first = json!({"videoId": "aaa", "name": "One"});
        let second = json!({"videoId": "bbb", "name": "Two"});
        let mut rows = remember(&first, &[], &path);
        rows = remember(&second, &rows, &path);
        rows = remember(
            &json!({"videoId": "aaa", "name": "One again"}),
            &rows,
            &path,
        );
        assert_eq!(
            rows.iter().map(video_id).collect::<Vec<_>>(),
            ["aaa", "bbb"]
        );
        assert!(rows[0]["externalUrl"].as_str().unwrap().ends_with("aaa"));
    }

    #[test]
    fn merge_keeps_local_ahead_of_remote() {
        let local = vec![json!({"videoId": "aaa", "name": "Local"})];
        let remote = vec![
            json!({"videoId": "bbb", "name": "Remote"}),
            json!({"videoId": "aaa", "name": "Old"}),
        ];
        let merged = merge(&local, &remote);
        assert_eq!(
            merged.iter().map(video_id).collect::<Vec<_>>(),
            ["aaa", "bbb"]
        );
        assert_eq!(merged[0]["name"], "Local");
    }
}
