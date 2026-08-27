use serde_json::{json, Map, Value};

pub fn as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

pub fn get_text(obj: &Value, key: &str) -> String {
    obj.get(key).map(as_text).unwrap_or_default()
}

pub fn first_object(value: &Value) -> Value {
    match value {
        Value::Object(_) => value.clone(),
        _ => json!({}),
    }
}

pub fn walk(value: &Value, visit: &mut impl FnMut(&Map<String, Value>)) {
    match value {
        Value::Object(map) => {
            visit(map);
            for child in map.values() {
                walk(child, visit);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, visit);
            }
        }
        _ => {}
    }
}

pub fn runs_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.trim().to_string();
    }
    if let Some(runs) = value.get("runs").and_then(Value::as_array) {
        return runs
            .iter()
            .map(|run| as_text(&run.get("text").cloned().unwrap_or(Value::Null)))
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();
    }
    if let Some(simple) = value.get("simpleText") {
        return as_text(simple);
    }
    String::new()
}

pub fn deep_get<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

pub fn find_first_str(value: &Value, key: &str) -> String {
    let mut found = String::new();
    walk(value, &mut |map| {
        if found.is_empty() {
            if let Some(Value::String(s)) = map.get(key) {
                if !s.is_empty() {
                    found = s.clone();
                }
            }
        }
    });
    found
}

pub fn collect_thumbnails(value: &Value) -> Vec<Value> {
    let mut thumbs = Vec::new();
    walk(value, &mut |map| {
        if let Some(Value::Array(items)) = map.get("thumbnails") {
            for item in items {
                if item.get("url").and_then(Value::as_str).is_some() {
                    thumbs.push(item.clone());
                }
            }
        }
    });
    thumbs
}
