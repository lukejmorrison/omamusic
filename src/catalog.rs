use crate::error::{Error, Result};
use crate::json_util::{as_text, get_text};
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const WATCH_BASE: &str = "https://music.youtube.com/watch?v=";
pub const PLAYLIST_BASE: &str = "https://music.youtube.com/playlist?list=";
pub const CHANNEL_BASE: &str = "https://music.youtube.com/channel/";
pub const BROWSE_BASE: &str = "https://music.youtube.com/browse/";

fn video_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{11}$").unwrap())
}

pub fn duration_ms(item: &Value) -> i64 {
    let source = item.as_object();
    if let Some(obj) = source {
        for key in ["duration_seconds", "lengthSeconds"] {
            if let Some(seconds) = obj.get(key) {
                if !as_text(seconds).is_empty() {
                    if let Ok(n) = as_text(seconds).parse::<f64>() {
                        return n.max(0.0) as i64 * 1000;
                    }
                }
            }
        }
        let text = {
            let duration = get_text(item, "duration");
            if duration.is_empty() {
                get_text(item, "length")
            } else {
                duration
            }
        };
        if text.is_empty() {
            return 0;
        }
        let mut total = 0i64;
        for part in text.split(':') {
            match part.parse::<i64>() {
                Ok(n) => total = total * 60 + n,
                Err(_) => return 0,
            }
        }
        return total * 1000;
    }
    0
}

pub fn thumbnail_url(item: &Value, min_width: i64) -> String {
    let mut thumbs = item
        .get("thumbnails")
        .cloned()
        .or_else(|| item.get("thumbnail").cloned());
    if let Some(Value::Object(map)) = &thumbs {
        thumbs = map
            .get("thumbnails")
            .cloned()
            .or_else(|| map.get("url").cloned());
    }
    if let Some(Value::String(url)) = &thumbs {
        return url.clone();
    }
    let Some(Value::Array(list)) = thumbs else {
        if let Some(album) = item.get("album") {
            if album.is_object() {
                return thumbnail_url(album, min_width);
            }
        }
        return String::new();
    };
    if list.is_empty() {
        if let Some(album) = item.get("album") {
            if album.is_object() {
                return thumbnail_url(album, min_width);
            }
        }
        return String::new();
    }
    let mut chosen = String::new();
    let mut chosen_width = -1i64;
    for thumb in list {
        if let Value::String(url) = &thumb {
            if !url.is_empty() {
                chosen = url.clone();
            }
            continue;
        }
        let Some(obj) = thumb.as_object() else {
            continue;
        };
        let url = obj.get("url").map(as_text).unwrap_or_default();
        if url.is_empty() {
            continue;
        }
        let width = obj
            .get("width")
            .and_then(Value::as_i64)
            .or_else(|| {
                as_text(obj.get("width").unwrap_or(&Value::Null))
                    .parse()
                    .ok()
            })
            .unwrap_or(0);
        if width >= min_width && width >= chosen_width {
            chosen = url;
            chosen_width = width;
        } else if chosen_width < 0 {
            chosen = url;
            chosen_width = width;
        }
    }
    chosen
}

pub fn artist_entries(item: &Value) -> Vec<Value> {
    let mut artists = item.get("artists").cloned();
    if artists.is_none() {
        artists = item.get("author").cloned();
    }
    if let Some(Value::Object(_)) = &artists {
        artists = Some(json!([artists.unwrap()]));
    }
    let Some(Value::Array(list)) = artists else {
        let name = get_text(item, "artist");
        return if name.is_empty() {
            vec![]
        } else {
            vec![json!({"id": "", "name": name, "type": "artist", "kind": "context"})]
        };
    };
    let mut result = Vec::new();
    for artist in list {
        if let Value::String(name) = &artist {
            let name = name.trim();
            if !name.is_empty() {
                result.push(json!({
                    "id": "",
                    "name": name,
                    "type": "artist",
                    "kind": "context",
                }));
            }
            continue;
        }
        let Some(obj) = artist.as_object() else {
            continue;
        };
        let name = obj.get("name").map(as_text).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let id = obj
            .get("id")
            .map(as_text)
            .filter(|s| !s.is_empty())
            .or_else(|| obj.get("channelId").map(as_text))
            .unwrap_or_default();
        result.push(json!({
            "id": id,
            "name": name,
            "type": "artist",
            "kind": "context",
            "uri": artist_uri(&id),
        }));
    }
    result
}

pub fn artist_names(item: &Value) -> String {
    artist_entries(item)
        .iter()
        .filter_map(|entry| entry.get("name").map(as_text))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn looks_like_video(value: &Value) -> bool {
    video_re().is_match(&as_text(value))
}

pub fn looks_like_video_str(value: &str) -> bool {
    video_re().is_match(value.trim())
}

fn looks_like_album_browse(value: &str) -> bool {
    value.starts_with("MPRE")
}

fn looks_like_playlist(value: &str) -> bool {
    value == "LM"
        || value.starts_with("PL")
        || value.starts_with("OL")
        || value.starts_with("RD")
        || value.starts_with("VL")
}

pub fn track_uri(video_id: &str) -> String {
    if video_id.is_empty() {
        String::new()
    } else {
        format!("ytm:track:{video_id}")
    }
}

pub fn playlist_uri(playlist_id: &str) -> String {
    if playlist_id.is_empty() {
        String::new()
    } else {
        format!("ytm:playlist:{playlist_id}")
    }
}

pub fn album_uri(album_id: &str) -> String {
    if album_id.is_empty() {
        String::new()
    } else {
        format!("ytm:album:{album_id}")
    }
}

pub fn artist_uri(artist_id: &str) -> String {
    if artist_id.is_empty() {
        String::new()
    } else {
        format!("ytm:artist:{artist_id}")
    }
}

pub fn watch_url(video_id: &str) -> String {
    if video_id.is_empty() {
        String::new()
    } else {
        format!("{WATCH_BASE}{video_id}")
    }
}

pub fn playlist_url(playlist_id: &str) -> String {
    if playlist_id.is_empty() {
        String::new()
    } else {
        format!("{PLAYLIST_BASE}{playlist_id}")
    }
}

pub fn browse_url(browse_id: &str) -> String {
    if browse_id.is_empty() {
        String::new()
    } else if browse_id.starts_with("UC") {
        format!("{CHANNEL_BASE}{browse_id}")
    } else if browse_id.starts_with("PL")
        || browse_id.starts_with("OL")
        || browse_id.starts_with("RD")
    {
        playlist_url(browse_id)
    } else {
        format!("{BROWSE_BASE}{browse_id}")
    }
}

pub fn liked_flag(item: &Value) -> bool {
    let status = get_text(item, "likeStatus");
    let status = if status.is_empty() {
        get_text(item, "like_status")
    } else {
        status
    };
    status.eq_ignore_ascii_case("LIKE")
}

pub fn video_id_of(item: &Value) -> String {
    let mut video_id = get_text(item, "videoId");
    if video_id.is_empty() {
        video_id = get_text(item, "video_id");
    }
    if !video_id.is_empty() {
        return video_id;
    }
    let candidate = item.get("id").cloned().unwrap_or(Value::Null);
    if looks_like_video(&candidate) {
        as_text(&candidate)
    } else {
        String::new()
    }
}

pub fn playlist_id_of(item: &Value) -> String {
    for key in ["playlistId", "audioPlaylistId", "browseId"] {
        let value = get_text(item, key);
        if !value.is_empty() {
            return value;
        }
    }
    let id = get_text(item, "id");
    if looks_like_video_str(&id) {
        String::new()
    } else {
        id
    }
}

pub fn album_item(item: &Value) -> Option<Value> {
    let album = item.get("album")?;
    if let Value::String(name) = album {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        return Some(json!({
            "kind": "context",
            "type": "album",
            "id": "",
            "uri": "",
            "name": name,
            "subtitle": artist_names(item),
            "imageUrl": thumbnail_url(item, 120),
        }));
    }
    let Value::Object(_) = album else { return None };
    let mut browse_id = {
        let id = get_text(album, "browseId");
        if id.is_empty() {
            get_text(album, "id")
        } else {
            id
        }
    };
    let mut playlist_id = {
        let id = get_text(album, "audioPlaylistId");
        if id.is_empty() {
            get_text(album, "playlistId")
        } else {
            id
        }
    };
    if looks_like_playlist(&browse_id) && !looks_like_album_browse(&browse_id) {
        if playlist_id.is_empty() {
            playlist_id = browse_id.clone();
        }
        browse_id.clear();
    }
    let mut name = get_text(album, "name");
    if name.is_empty() {
        name = get_text(album, "title");
    }
    if name.is_empty() && browse_id.is_empty() && playlist_id.is_empty() {
        return None;
    }
    let image = {
        let url = thumbnail_url(album, 120);
        if url.is_empty() {
            thumbnail_url(item, 120)
        } else {
            url
        }
    };
    let uri = {
        let album = album_uri(&browse_id);
        if album.is_empty() {
            playlist_uri(&playlist_id)
        } else {
            album
        }
    };
    let external = {
        let browse = browse_url(&browse_id);
        if browse.is_empty() {
            playlist_url(&playlist_id)
        } else {
            browse
        }
    };
    Some(json!({
        "kind": "context",
        "type": "album",
        "id": browse_id,
        "uri": uri,
        "name": if name.is_empty() { "Album".into() } else { name },
        "subtitle": artist_names(item),
        "imageUrl": image,
        "playlistId": playlist_id,
        "externalUrl": external,
    }))
}

pub fn attach_album_context(track: &mut Value, parent: &Value) {
    if !track.is_object() || !parent.is_object() {
        return;
    }
    let current_album = get_text(track, "album");
    let current_image = get_text(track, "imageUrl");
    let mut merged = match track.get("albumItem") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        _ => json!({}),
    };
    if let Some(merged_obj) = merged.as_object_mut() {
        for key in ["id", "uri", "name", "playlistId", "imageUrl", "subtitle"] {
            let current = merged_obj.get(key).map(as_text).unwrap_or_default();
            let parent_val = get_text(parent, key);
            if current.is_empty() && !parent_val.is_empty() {
                merged_obj.insert(key.into(), json!(parent_val));
            }
        }
        merged_obj.insert("kind".into(), json!("context"));
        merged_obj.insert("type".into(), json!("album"));
        if as_text(merged_obj.get("uri").unwrap_or(&Value::Null)).is_empty() {
            let id = as_text(merged_obj.get("id").unwrap_or(&Value::Null));
            let playlist = as_text(merged_obj.get("playlistId").unwrap_or(&Value::Null));
            let uri = album_uri(&id);
            merged_obj.insert(
                "uri".into(),
                json!(if uri.is_empty() {
                    playlist_uri(&playlist)
                } else {
                    uri
                }),
            );
        }
    }
    let merged_name = get_text(&merged, "name");
    let parent_name = get_text(parent, "name");
    let parent_image = get_text(parent, "imageUrl");
    if let Some(obj) = track.as_object_mut() {
        if current_album.is_empty() {
            let name = if merged_name.is_empty() {
                parent_name
            } else {
                merged_name
            };
            obj.insert("album".into(), json!(name));
        }
        if current_image.is_empty() && !parent_image.is_empty() {
            obj.insert("imageUrl".into(), json!(parent_image));
        }
        obj.insert("albumItem".into(), merged);
    }
}

pub fn track_item(raw: &Value, fallback_type: &str) -> Option<Value> {
    let Value::Object(_) = raw else { return None };
    let mut result_type = get_text(raw, "resultType");
    if result_type.is_empty() {
        result_type = get_text(raw, "type");
    }
    if result_type.is_empty() {
        result_type = fallback_type.to_string();
    }
    let result_type = result_type.to_ascii_lowercase();
    if result_type == "artist" {
        return context_item(raw, "artist");
    }
    if matches!(result_type.as_str(), "album" | "ep" | "single") {
        return context_item(raw, "album");
    }
    if result_type == "playlist" {
        return context_item(raw, "playlist");
    }
    let mut video_id = get_text(raw, "videoId");
    if video_id.is_empty() {
        video_id = get_text(raw, "video_id");
    }
    if video_id.is_empty()
        && looks_like_video(raw.get("id").unwrap_or(&Value::Null))
        && matches!(result_type.as_str(), "" | "song" | "video" | "track")
    {
        video_id = get_text(raw, "id");
    }
    let mut title = get_text(raw, "title");
    if title.is_empty() {
        title = get_text(raw, "name");
    }
    if title.is_empty() {
        return None;
    }
    if video_id.is_empty() {
        if matches!(result_type.as_str(), "album" | "playlist" | "artist") {
            return context_item(raw, &result_type);
        }
        return None;
    }
    let artists = artist_entries(raw);
    let album = album_item(raw);
    let duration = duration_ms(raw);
    let playlist_id = get_text(raw, "playlistId");
    Some(json!({
        "kind": "item",
        "type": "track",
        "id": video_id,
        "uri": track_uri(&video_id),
        "name": title,
        "subtitle": artist_names(raw),
        "album": album.as_ref().and_then(|a| a.get("name").map(as_text)).unwrap_or_default(),
        "artists": artists,
        "albumItem": album,
        "imageUrl": thumbnail_url(raw, 120),
        "durationMs": duration,
        "videoId": video_id,
        "playlistId": playlist_id,
        "externalUrl": watch_url(&video_id),
        "liked": liked_flag(raw),
        "setVideoId": get_text(raw, "setVideoId"),
    }))
}

pub fn context_item(raw: &Value, forced_type: &str) -> Option<Value> {
    let Value::Object(_) = raw else { return None };
    let mut item_type = if forced_type.is_empty() {
        let mut t = get_text(raw, "resultType");
        if t.is_empty() {
            t = get_text(raw, "type");
        }
        t.to_ascii_lowercase()
    } else {
        forced_type.to_ascii_lowercase()
    };
    if matches!(item_type.as_str(), "ep" | "single") {
        item_type = "album".into();
    }
    if !matches!(item_type.as_str(), "album" | "artist" | "playlist") {
        if raw.get("browseId").is_some() && raw.get("artists").is_some() {
            item_type = "album".into();
        } else if raw.get("subscribers").is_some() || raw.get("channelId").is_some() {
            item_type = "artist".into();
        } else if raw.get("playlistId").is_some() || get_text(raw, "id").starts_with("PL") {
            item_type = "playlist".into();
        } else {
            return None;
        }
    }
    let mut name = get_text(raw, "title");
    if name.is_empty() {
        name = get_text(raw, "name");
    }
    if name.is_empty() {
        name = get_text(raw, "artist");
    }
    if name.is_empty() {
        return None;
    }
    let mut item_id = String::new();
    for key in [
        "browseId",
        "channelId",
        "playlistId",
        "audioPlaylistId",
        "id",
    ] {
        item_id = get_text(raw, key);
        if !item_id.is_empty() {
            break;
        }
    }
    let uri = match item_type.as_str() {
        "album" => album_uri(&item_id),
        "artist" => artist_uri(&item_id),
        _ => playlist_uri(&item_id),
    };
    let mut subtitle = artist_names(raw);
    if item_type == "artist" {
        subtitle = get_text(raw, "subscribers");
        if subtitle.is_empty() {
            subtitle = get_text(raw, "subtitle");
        }
    } else if item_type == "playlist" {
        subtitle = match raw.get("author") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Object(map)) => map.get("name").map(as_text).unwrap_or_default(),
            _ => subtitle,
        };
        let count = raw
            .get("count")
            .or_else(|| raw.get("trackCount"))
            .map(as_text)
            .unwrap_or_default();
        if !count.is_empty() {
            subtitle = if subtitle.is_empty() {
                format!("{count} songs")
            } else {
                format!("{subtitle} · {count} songs")
            };
        }
    }
    let playlist_id = if item_type == "playlist" {
        item_id.clone()
    } else {
        let id = get_text(raw, "audioPlaylistId");
        if id.is_empty() {
            get_text(raw, "playlistId")
        } else {
            id
        }
    };
    let external = {
        let browse = browse_url(&item_id);
        if browse.is_empty() {
            playlist_url(&item_id)
        } else {
            browse
        }
    };
    Some(json!({
        "kind": "context",
        "type": item_type,
        "id": item_id,
        "uri": uri,
        "name": name,
        "subtitle": subtitle,
        "album": if item_type == "album" { name.clone() } else { String::new() },
        "artists": artist_entries(raw),
        "imageUrl": thumbnail_url(raw, 120),
        "durationMs": duration_ms(raw),
        "videoId": "",
        "playlistId": playlist_id,
        "externalUrl": external,
        "liked": false,
        "year": get_text(raw, "year"),
        "description": get_text(raw, "description"),
    }))
}

pub fn normalize_item(raw: &Value) -> Option<Value> {
    let Value::Object(_) = raw else { return None };
    let mut result_type = get_text(raw, "resultType");
    if result_type.is_empty() {
        result_type = get_text(raw, "type");
    }
    let result_type = result_type.to_ascii_lowercase();
    if matches!(
        result_type.as_str(),
        "artist" | "album" | "playlist" | "ep" | "single"
    ) {
        let forced = if matches!(result_type.as_str(), "ep" | "single") {
            "album"
        } else {
            &result_type
        };
        return context_item(raw, forced);
    }
    if raw.get("videoId").is_some() || looks_like_video(raw.get("id").unwrap_or(&Value::Null)) {
        return track_item(raw, "track");
    }
    context_item(raw, "").or_else(|| track_item(raw, "track"))
}

pub fn map_items(values: &Value, limit: usize) -> Vec<Value> {
    let list = match values {
        Value::Object(map) => map
            .get("tracks")
            .or_else(|| map.get("results"))
            .or_else(|| map.get("contents"))
            .or_else(|| map.get("items")),
        Value::Array(_) => Some(values),
        _ => None,
    };
    let Some(Value::Array(items)) = list else {
        return vec![];
    };
    let mut out = Vec::new();
    for raw in items {
        if let Some(item) = normalize_item(raw) {
            out.push(item);
            if limit > 0 && out.len() >= limit {
                break;
            }
        }
    }
    out
}

pub fn shelf_from_home(raw: &Value, track_limit: usize) -> Option<Value> {
    let Value::Object(_) = raw else { return None };
    let title = get_text(raw, "title");
    let tracks = map_items(raw.get("contents").unwrap_or(&Value::Null), track_limit);
    if tracks.is_empty() {
        return None;
    }
    Some(json!({
        "title": if title.is_empty() { "Home" } else { &title },
        "tracks": tracks,
    }))
}

pub fn looks_unauthorized(exc: &str) -> bool {
    let text = exc.to_ascii_lowercase();
    text.contains("401")
        || text.contains("unauthorized")
        || text.contains("must be signed in")
        || text.contains("please sign in")
        || text.contains("not logged in")
}

pub fn raise_auth_or_catalog(exc: &str, action: &str) -> Error {
    if looks_unauthorized(exc) {
        Error::auth_required(format!("Sign in to {action}"))
    } else {
        Error::catalog(exc)
    }
}

pub trait CatalogOps: Send {
    fn account(&mut self) -> Value;
    fn home(&mut self, limit: usize, force: bool) -> Vec<Value>;
    fn history(&mut self, limit: usize) -> Result<Vec<Value>>;
    fn liked(&mut self, limit: usize) -> Result<Vec<Value>>;
    fn playlists(&mut self, limit: usize) -> Result<Vec<Value>>;
    fn library_songs(&mut self, limit: usize) -> Result<Vec<Value>>;
    fn library_albums(&mut self, limit: usize) -> Result<Vec<Value>>;
    fn library_artists(&mut self, limit: usize) -> Result<Vec<Value>>;
    fn search(&mut self, query: &str, filter_name: &str, limit: usize) -> Value;
    fn playlist(&mut self, playlist_id: &str, limit: usize) -> Result<Value>;
    fn album(&mut self, album_id: &str) -> Result<Value>;
    fn artist(&mut self, artist_id: &str) -> Result<Value>;
    fn watch_playlist(&mut self, video_id: &str, limit: usize) -> Vec<Value>;
    fn rate_song(&mut self, video_id: &str, liked: bool) -> Result<()>;
    fn rate_playlist(&mut self, playlist_id: &str, liked: bool) -> Result<()>;
    fn create_playlist(&mut self, name: &str) -> Result<Value>;
    fn add_to_playlist(&mut self, playlist_id: &str, video_id: &str) -> Result<()>;
}

impl<C: YtmClient> CatalogOps for Catalog<C> {
    fn account(&mut self) -> Value {
        Catalog::account(self)
    }
    fn home(&mut self, limit: usize, force: bool) -> Vec<Value> {
        Catalog::home(self, limit, force)
    }
    fn history(&mut self, limit: usize) -> Result<Vec<Value>> {
        Catalog::history(self, limit)
    }
    fn liked(&mut self, limit: usize) -> Result<Vec<Value>> {
        Catalog::liked(self, limit)
    }
    fn playlists(&mut self, limit: usize) -> Result<Vec<Value>> {
        Catalog::playlists(self, limit)
    }
    fn library_songs(&mut self, limit: usize) -> Result<Vec<Value>> {
        Catalog::library_songs(self, limit)
    }
    fn library_albums(&mut self, limit: usize) -> Result<Vec<Value>> {
        Catalog::library_albums(self, limit)
    }
    fn library_artists(&mut self, limit: usize) -> Result<Vec<Value>> {
        Catalog::library_artists(self, limit)
    }
    fn search(&mut self, query: &str, filter_name: &str, limit: usize) -> Value {
        Catalog::search(self, query, filter_name, limit)
    }
    fn playlist(&mut self, playlist_id: &str, limit: usize) -> Result<Value> {
        Catalog::playlist(self, playlist_id, limit)
    }
    fn album(&mut self, album_id: &str) -> Result<Value> {
        Catalog::album(self, album_id)
    }
    fn artist(&mut self, artist_id: &str) -> Result<Value> {
        Catalog::artist(self, artist_id)
    }
    fn watch_playlist(&mut self, video_id: &str, limit: usize) -> Vec<Value> {
        Catalog::watch_playlist(self, video_id, limit)
    }
    fn rate_song(&mut self, video_id: &str, liked: bool) -> Result<()> {
        Catalog::rate_song(self, video_id, liked)
    }
    fn rate_playlist(&mut self, playlist_id: &str, liked: bool) -> Result<()> {
        Catalog::rate_playlist(self, playlist_id, liked)
    }
    fn create_playlist(&mut self, name: &str) -> Result<Value> {
        Catalog::create_playlist(self, name)
    }
    fn add_to_playlist(&mut self, playlist_id: &str, video_id: &str) -> Result<()> {
        Catalog::add_to_playlist(self, playlist_id, video_id)
    }
}

pub trait YtmClient: Send + Sync {
    fn get_account_info(&self) -> Result<Value>;
    fn get_home(&self, limit: usize) -> Result<Vec<Value>>;
    fn get_history(&self) -> Result<Vec<Value>>;
    fn get_liked_songs(&self, limit: usize) -> Result<Value>;
    fn get_library_playlists(&self, limit: usize) -> Result<Vec<Value>>;
    fn get_library_songs(&self, limit: usize) -> Result<Vec<Value>>;
    fn get_library_albums(&self, limit: usize) -> Result<Vec<Value>>;
    fn get_library_artists(&self, limit: usize) -> Result<Vec<Value>>;
    fn search(&self, query: &str, filter: Option<&str>, limit: usize) -> Result<Vec<Value>>;
    fn get_playlist(&self, playlist_id: &str, limit: usize) -> Result<Value>;
    fn get_album(&self, album_id: &str) -> Result<Value>;
    fn get_artist(&self, artist_id: &str) -> Result<Value>;
    fn get_watch_playlist(&self, video_id: &str, limit: usize) -> Result<Value>;
    fn rate_song(&self, video_id: &str, rating: &str) -> Result<()>;
    fn rate_playlist(&self, playlist_id: &str, rating: &str) -> Result<()>;
    fn create_playlist(&self, name: &str) -> Result<String>;
    fn add_playlist_items(&self, playlist_id: &str, video_id: &str) -> Result<()>;
}

pub struct Catalog<C: YtmClient> {
    pub yt: C,
    home: Option<Vec<Value>>,
    home_at: Option<Instant>,
    home_ttl: Duration,
}

impl<C: YtmClient> Catalog<C> {
    pub fn new(yt: C) -> Self {
        Self {
            yt,
            home: None,
            home_at: None,
            home_ttl: Duration::from_secs(120),
        }
    }

    pub fn account(&self) -> Value {
        let info = self.yt.get_account_info().unwrap_or_else(|_| json!({}));
        let mut name = get_text(&info, "accountName");
        if name.is_empty() {
            name = get_text(&info, "name");
        }
        if name.is_empty() {
            name = info
                .get("account")
                .and_then(|a| a.get("name").map(as_text))
                .unwrap_or_default();
        }
        json!({"name": name, "raw": info})
    }

    pub fn home(&mut self, limit: usize, force: bool) -> Vec<Value> {
        let now = Instant::now();
        if !force {
            if let (Some(home), Some(at)) = (&self.home, self.home_at) {
                if now.duration_since(at) < self.home_ttl {
                    return home.clone();
                }
            }
        }
        let shelves = self.yt.get_home(limit).unwrap_or_default();
        let mut out = Vec::new();
        for shelf in shelves {
            if let Some(mapped) = shelf_from_home(&shelf, 12) {
                out.push(mapped);
                if out.len() >= limit {
                    break;
                }
            }
        }
        self.home = Some(out.clone());
        self.home_at = Some(now);
        out
    }

    pub fn history(&self, limit: usize) -> Result<Vec<Value>> {
        match self.yt.get_history() {
            Ok(raw) => Ok(map_items(&Value::Array(raw), limit)),
            Err(Error::AuthRequired(msg)) => Err(Error::auth_required(msg)),
            Err(err) => {
                let text = err.to_string();
                if looks_unauthorized(&text) {
                    Err(raise_auth_or_catalog(&text, "see history"))
                } else {
                    Ok(vec![])
                }
            }
        }
    }

    pub fn liked(&self, limit: usize) -> Result<Vec<Value>> {
        match self.yt.get_liked_songs(limit) {
            Ok(raw) => Ok(map_items(&raw, limit)),
            Err(err) => Err(raise_auth_or_catalog(&err.to_string(), "see liked songs")),
        }
    }

    pub fn playlists(&self, limit: usize) -> Result<Vec<Value>> {
        match self.yt.get_library_playlists(limit) {
            Ok(raw) => Ok(map_items(&Value::Array(raw), limit)
                .into_iter()
                .filter(|item| {
                    get_text(item, "type") == "playlist" || get_text(item, "kind") == "context"
                })
                .collect()),
            Err(err) => Err(raise_auth_or_catalog(&err.to_string(), "see playlists")),
        }
    }

    pub fn library_songs(&self, limit: usize) -> Result<Vec<Value>> {
        match self.yt.get_library_songs(limit) {
            Ok(raw) => Ok(map_items(&Value::Array(raw), limit)),
            Err(err) => Err(raise_auth_or_catalog(&err.to_string(), "see your library")),
        }
    }

    pub fn library_albums(&self, limit: usize) -> Result<Vec<Value>> {
        match self.yt.get_library_albums(limit) {
            Ok(raw) => Ok(map_items(&Value::Array(raw), limit)),
            Err(err) => Err(raise_auth_or_catalog(&err.to_string(), "see your library")),
        }
    }

    pub fn library_artists(&self, limit: usize) -> Result<Vec<Value>> {
        match self.yt.get_library_artists(limit) {
            Ok(raw) => Ok(map_items(&Value::Array(raw), limit)),
            Err(err) => Err(raise_auth_or_catalog(&err.to_string(), "see your library")),
        }
    }

    pub fn search(&self, query: &str, filter_name: &str, limit: usize) -> Value {
        let query = query.trim();
        if query.is_empty() {
            return json!({"items": [], "sections": []});
        }
        let mut filter_name = filter_name.trim().to_ascii_lowercase();
        filter_name = match filter_name.as_str() {
            "track" | "song" | "songs" => "songs".into(),
            "album" => "albums".into(),
            "artist" => "artists".into(),
            "playlist" => "playlists".into(),
            other => other.into(),
        };
        if matches!(
            filter_name.as_str(),
            "songs" | "albums" | "artists" | "playlists" | "videos"
        ) {
            let raw = self
                .yt
                .search(query, Some(&filter_name), limit)
                .unwrap_or_default();
            let items = map_items(&Value::Array(raw), limit);
            let title = match filter_name.as_str() {
                "songs" => "Songs",
                "albums" => "Albums",
                "artists" => "Artists",
                "playlists" => "Playlists",
                _ => "Videos",
            };
            return json!({
                "items": items,
                "sections": [{"title": title, "items": items}],
            });
        }
        let mut sections = Vec::new();
        let mut all_items = Vec::new();
        for (name, title) in [
            ("songs", "Songs"),
            ("albums", "Albums"),
            ("artists", "Artists"),
            ("playlists", "Playlists"),
        ] {
            let raw = self
                .yt
                .search(query, Some(name), limit.min(8))
                .unwrap_or_default();
            let items = map_items(&Value::Array(raw), limit.min(8));
            if !items.is_empty() {
                all_items.extend(items.clone());
                sections.push(json!({"title": title, "items": items}));
            }
        }
        json!({"items": all_items, "sections": sections})
    }

    pub fn playlist(&self, playlist_id: &str, limit: usize) -> Result<Value> {
        let playlist_id = playlist_id.trim();
        let raw = self
            .yt
            .get_playlist(playlist_id, limit)
            .map_err(|err| Error::catalog(err.to_string()))?;
        let playlist_name = {
            let t = get_text(&raw, "title");
            if t.is_empty() {
                "Playlist".into()
            } else {
                t
            }
        };
        let mut header = context_item(&raw, "playlist").unwrap_or_else(|| {
            json!({
                "kind": "context",
                "type": "playlist",
                "id": playlist_id,
                "uri": playlist_uri(playlist_id),
                "name": playlist_name,
                "subtitle": "",
                "imageUrl": thumbnail_url(&raw, 120),
                "externalUrl": playlist_url(playlist_id),
                "playlistId": playlist_id,
            })
        });
        if get_text(&header, "id").is_empty() {
            header["id"] = json!(playlist_id);
        }
        let pid = get_text(&header, "playlistId");
        header["playlistId"] = json!(if pid.is_empty() { playlist_id } else { &pid });
        let hid = get_text(&header, "id");
        let uri = playlist_uri(&hid);
        header["uri"] = json!(if uri.is_empty() {
            playlist_uri(playlist_id)
        } else {
            uri
        });
        if get_text(&header, "externalUrl").is_empty() {
            let hid = get_text(&header, "id");
            header["externalUrl"] = json!(playlist_url(if hid.is_empty() {
                playlist_id
            } else {
                &hid
            }));
        }
        let tracks = map_items(raw.get("tracks").unwrap_or(&Value::Null), limit);
        header["tracks"] = json!(tracks);
        Ok(header)
    }

    pub fn album(&self, album_id: &str) -> Result<Value> {
        let album_id = album_id.trim();
        let raw = self
            .yt
            .get_album(album_id)
            .map_err(|err| Error::catalog(err.to_string()))?;
        let album_name = {
            let t = get_text(&raw, "title");
            if t.is_empty() {
                "Album".into()
            } else {
                t
            }
        };
        let mut header = context_item(&raw, "album").unwrap_or_else(|| {
            json!({
                "kind": "context",
                "type": "album",
                "id": album_id,
                "uri": album_uri(album_id),
                "name": album_name,
                "subtitle": artist_names(&raw),
                "imageUrl": thumbnail_url(&raw, 120),
            })
        });
        let mut playlist_id = get_text(&raw, "audioPlaylistId");
        if playlist_id.is_empty() {
            playlist_id = get_text(&raw, "playlistId");
        }
        if playlist_id.is_empty() {
            playlist_id = get_text(&header, "playlistId");
        }
        if looks_like_album_browse(album_id) {
            header["id"] = json!(album_id);
            header["uri"] = json!(album_uri(album_id));
            let external = browse_url(album_id);
            if !external.is_empty() {
                header["externalUrl"] = json!(external);
            }
        } else if get_text(&header, "id").is_empty() {
            header["id"] = json!(album_id);
        }
        header["playlistId"] = json!(playlist_id);
        let header_id = get_text(&header, "id");
        let header_uri = get_text(&header, "uri");
        let parent_album = json!({
            "kind": "context",
            "type": "album",
            "id": header_id,
            "uri": if header_uri.is_empty() { album_uri(&get_text(&header, "id")) } else { header_uri },
            "name": get_text(&header, "name"),
            "playlistId": playlist_id,
            "imageUrl": get_text(&header, "imageUrl"),
            "subtitle": get_text(&header, "subtitle"),
        });
        let mut tracks = map_items(raw.get("tracks").unwrap_or(&Value::Null), 0);
        for track in &mut tracks {
            attach_album_context(track, &parent_album);
        }
        header["tracks"] = json!(tracks);
        Ok(header)
    }

    pub fn artist(&self, artist_id: &str) -> Result<Value> {
        let artist_id = artist_id.trim();
        let raw = self
            .yt
            .get_artist(artist_id)
            .map_err(|err| Error::catalog(err.to_string()))?;
        let artist_name = {
            let t = get_text(&raw, "name");
            if t.is_empty() {
                "Artist".into()
            } else {
                t
            }
        };
        let mut header = context_item(&raw, "artist").unwrap_or_else(|| {
            json!({
                "kind": "context",
                "type": "artist",
                "id": artist_id,
                "uri": artist_uri(artist_id),
                "name": artist_name,
                "subtitle": get_text(&raw, "subscribers"),
                "imageUrl": thumbnail_url(&raw, 120),
                "description": get_text(&raw, "description"),
            })
        });
        let songs = map_items(
            raw.get("songs")
                .and_then(|s| s.get("results"))
                .unwrap_or(&json!([])),
            20,
        );
        let albums = map_items(
            raw.get("albums")
                .and_then(|s| s.get("results"))
                .unwrap_or(&json!([])),
            20,
        );
        let singles = map_items(
            raw.get("singles")
                .and_then(|s| s.get("results"))
                .unwrap_or(&json!([])),
            12,
        );
        header["tracks"] = json!(songs);
        header["albums"] = json!(albums);
        header["singles"] = json!(singles);
        header["shuffleId"] = json!(get_text(&raw, "shuffleId"));
        header["radioId"] = json!(get_text(&raw, "radioId"));
        Ok(header)
    }

    pub fn watch_playlist(&self, video_id: &str, limit: usize) -> Vec<Value> {
        match self.yt.get_watch_playlist(video_id.trim(), limit) {
            Ok(raw) => map_items(raw.get("tracks").unwrap_or(&Value::Null), limit),
            Err(_) => vec![],
        }
    }

    pub fn rate_song(&self, video_id: &str, liked: bool) -> Result<()> {
        let rating = if liked { "LIKE" } else { "INDIFFERENT" };
        self.yt
            .rate_song(video_id, rating)
            .map_err(|err| raise_auth_or_catalog(&err.to_string(), "like songs"))
    }

    pub fn rate_playlist(&self, playlist_id: &str, liked: bool) -> Result<()> {
        let rating = if liked { "LIKE" } else { "INDIFFERENT" };
        self.yt
            .rate_playlist(playlist_id, rating)
            .map_err(|err| raise_auth_or_catalog(&err.to_string(), "like albums"))
    }

    pub fn create_playlist(&self, name: &str) -> Result<Value> {
        let name = {
            let n = name.trim();
            if n.is_empty() {
                "New playlist"
            } else {
                n
            }
        };
        let playlist_id = self
            .yt
            .create_playlist(name)
            .map_err(|err| raise_auth_or_catalog(&err.to_string(), "create playlists"))?;
        Ok(json!({
            "kind": "context",
            "type": "playlist",
            "id": playlist_id,
            "uri": playlist_uri(&playlist_id),
            "name": name,
            "subtitle": "Your playlist",
            "playlistId": playlist_id,
            "externalUrl": playlist_url(&playlist_id),
        }))
    }

    pub fn add_to_playlist(&self, playlist_id: &str, video_id: &str) -> Result<()> {
        self.yt
            .add_playlist_items(playlist_id, video_id)
            .map_err(|err| raise_auth_or_catalog(&err.to_string(), "save to playlists"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockYt {
        home: Mutex<Vec<Value>>,
        home_calls: Mutex<usize>,
        rate_err: Option<String>,
        liked_err: Option<String>,
        playlists_err: Option<String>,
        playlist: Mutex<Option<(String, usize, Value)>>,
        album: Mutex<Option<(String, Value)>>,
    }

    impl Default for MockYt {
        fn default() -> Self {
            Self {
                home: Mutex::new(vec![]),
                home_calls: Mutex::new(0),
                rate_err: None,
                liked_err: None,
                playlists_err: None,
                playlist: Mutex::new(None),
                album: Mutex::new(None),
            }
        }
    }

    impl YtmClient for MockYt {
        fn get_account_info(&self) -> Result<Value> {
            Ok(json!({}))
        }
        fn get_home(&self, _limit: usize) -> Result<Vec<Value>> {
            *self.home_calls.lock().unwrap() += 1;
            Ok(self.home.lock().unwrap().clone())
        }
        fn get_history(&self) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn get_liked_songs(&self, _limit: usize) -> Result<Value> {
            if let Some(err) = &self.liked_err {
                return Err(Error::catalog(err.clone()));
            }
            Ok(json!([]))
        }
        fn get_library_playlists(&self, _limit: usize) -> Result<Vec<Value>> {
            if let Some(err) = &self.playlists_err {
                return Err(Error::catalog(err.clone()));
            }
            Ok(vec![])
        }
        fn get_library_songs(&self, _limit: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn get_library_albums(&self, _limit: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn get_library_artists(&self, _limit: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn search(&self, _q: &str, _f: Option<&str>, _l: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn get_playlist(&self, playlist_id: &str, limit: usize) -> Result<Value> {
            *self.playlist.lock().unwrap() = Some((
                playlist_id.to_string(),
                limit,
                json!({"title": "Bangers", "tracks": []}),
            ));
            if playlist_id == "VLPLQ6abc" {
                return Ok(json!({
                    "title": "Kate Sutherland",
                    "tracks": [{"title": "Song", "videoId": "abcdefghijk"}],
                }));
            }
            if playlist_id == "PLabc" {
                return Ok(json!({"title": "Bangers", "tracks": []}));
            }
            Ok(json!({"title": "Bangers", "tracks": []}))
        }
        fn get_album(&self, album_id: &str) -> Result<Value> {
            let value = json!({
                "title": "Rooted Through Darkness",
                "artists": [{"name": "Kate Sutherland", "id": "UC123"}],
                "audioPlaylistId": "OLAK5uy_abc",
                "thumbnails": [{"url": "https://img/cover.jpg", "width": 226}],
                "tracks": [{
                    "title": "Glorious Day",
                    "videoId": "abcdefghijk",
                    "album": "Rooted Through Darkness",
                    "artists": [{"name": "Kate Sutherland", "id": "UC123"}],
                }],
            });
            *self.album.lock().unwrap() = Some((album_id.to_string(), value.clone()));
            Ok(value)
        }
        fn get_artist(&self, _id: &str) -> Result<Value> {
            Ok(json!({}))
        }
        fn get_watch_playlist(&self, _id: &str, _l: usize) -> Result<Value> {
            Ok(json!({"tracks": []}))
        }
        fn rate_song(&self, _id: &str, _rating: &str) -> Result<()> {
            if let Some(err) = &self.rate_err {
                return Err(Error::catalog(err.clone()));
            }
            Ok(())
        }
        fn rate_playlist(&self, _id: &str, _rating: &str) -> Result<()> {
            Ok(())
        }
        fn create_playlist(&self, _name: &str) -> Result<String> {
            Ok("PLnew".into())
        }
        fn add_playlist_items(&self, _p: &str, _v: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn duration_parses_clock_and_seconds() {
        assert_eq!(duration_ms(&json!({"duration": "3:45"})), 225000);
        assert_eq!(duration_ms(&json!({"duration": "1:02:03"})), 3723000);
        assert_eq!(duration_ms(&json!({"duration_seconds": 90})), 90000);
        assert_eq!(duration_ms(&json!({})), 0);
    }

    #[test]
    fn track_item_normalizes_song() {
        let item = track_item(
            &json!({
                "title": "Under the Bridge",
                "videoId": "GLvqBAudoEg",
                "artists": [{"name": "Red Hot Chili Peppers", "id": "UC123"}],
                "album": {"name": "Blood Sugar Sex Magik", "id": "MPREb_album"},
                "duration": "4:24",
                "thumbnails": [
                    {"url": "https://img/small.jpg", "width": 60},
                    {"url": "https://img/large.jpg", "width": 544}
                ],
                "likeStatus": "LIKE",
            }),
            "track",
        )
        .unwrap();
        assert_eq!(item["type"], "track");
        assert_eq!(item["kind"], "item");
        assert_eq!(item["uri"], "ytm:track:GLvqBAudoEg");
        assert_eq!(item["subtitle"], "Red Hot Chili Peppers");
        assert_eq!(item["album"], "Blood Sugar Sex Magik");
        assert_eq!(item["liked"], true);
        assert_eq!(item["imageUrl"], "https://img/large.jpg");
        assert_eq!(item["durationMs"], 264000);
        assert_eq!(item["albumItem"]["type"], "album");
        assert_eq!(item["albumItem"]["id"], "MPREb_album");
        assert!(item["albumItem"]["externalUrl"]
            .as_str()
            .unwrap()
            .contains("browse/MPREb_album"));
    }

    #[test]
    fn album_item_keeps_playlist_id() {
        let item = track_item(
            &json!({
                "title": "Song",
                "videoId": "abcdefghijk",
                "album": {
                    "name": "Record",
                    "id": "MPREb_record",
                    "audioPlaylistId": "OLAK5uy_abc",
                },
            }),
            "track",
        )
        .unwrap();
        assert_eq!(item["albumItem"]["playlistId"], "OLAK5uy_abc");
        assert!(item["albumItem"]["externalUrl"]
            .as_str()
            .unwrap()
            .contains("browse/MPREb_record"));
    }

    #[test]
    fn context_item_playlist() {
        let item = context_item(
            &json!({
                "title": "Liked Music",
                "playlistId": "LM",
                "count": 12,
            }),
            "playlist",
        )
        .unwrap();
        assert_eq!(item["type"], "playlist");
        assert_eq!(item["kind"], "context");
        assert!(item["subtitle"].as_str().unwrap().contains("12 songs"));
    }

    #[test]
    fn map_items_skips_junk() {
        let rows = map_items(
            &json!([
                null,
                {"title": "Nope"},
                {"title": "Song", "videoId": "abcdefghijk"},
            ]),
            0,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["videoId"], "abcdefghijk");
    }

    #[test]
    fn thumbnail_prefers_wide_image() {
        let url = thumbnail_url(
            &json!({
                "thumbnails": [
                    {"url": "a", "width": 60},
                    {"url": "b", "width": 226},
                ]
            }),
            120,
        );
        assert_eq!(url, "b");
    }

    #[test]
    fn looks_unauthorized_catches_ytmusicapi_401() {
        assert!(looks_unauthorized(
            "Server returned HTTP 401: Unauthorized. You must be signed in to perform this operation."
        ));
        assert!(!looks_unauthorized("connection timed out"));
    }

    #[test]
    fn home_caches_until_forced() {
        let yt = MockYt {
            home: Mutex::new(vec![json!({
                "title": "That summer feeling",
                "contents": [{"title": "Song", "videoId": "abcdefghijk"}],
            })]),
            ..MockYt::default()
        };
        let mut catalog = Catalog::new(yt);
        let first = catalog.home(6, false);
        let second = catalog.home(6, false);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0]["title"], "That summer feeling");
        assert_eq!(first, second);
        assert_eq!(*catalog.yt.home_calls.lock().unwrap(), 1);
        catalog.home(6, true);
        assert_eq!(*catalog.yt.home_calls.lock().unwrap(), 2);
    }

    #[test]
    fn rate_song_maps_401_to_sign_in() {
        let yt = MockYt {
            rate_err: Some(
                "Server returned HTTP 401: Unauthorized. You must be signed in to perform this operation."
                    .into(),
            ),
            ..MockYt::default()
        };
        let catalog = Catalog::new(yt);
        let err = catalog.rate_song("abcdefghijk", true).unwrap_err();
        assert_eq!(err.to_string(), "Sign in to like songs");
        assert!(matches!(err, Error::AuthRequired(_)));
    }

    #[test]
    fn liked_maps_401_to_sign_in() {
        let yt = MockYt {
            liked_err: Some(
                "Server returned HTTP 401: Unauthorized. You must be signed in to perform this operation."
                    .into(),
            ),
            ..MockYt::default()
        };
        let err = Catalog::new(yt).liked(50).unwrap_err();
        assert_eq!(err.to_string(), "Sign in to see liked songs");
    }

    #[test]
    fn playlists_maps_401_to_sign_in() {
        let yt = MockYt {
            playlists_err: Some("Server returned HTTP 401: Unauthorized.".into()),
            ..MockYt::default()
        };
        let err = Catalog::new(yt).playlists(50).unwrap_err();
        assert_eq!(err.to_string(), "Sign in to see playlists");
    }

    #[test]
    fn liked_does_not_hide_other_errors() {
        let yt = MockYt {
            liked_err: Some("connection timed out".into()),
            ..MockYt::default()
        };
        let err = Catalog::new(yt).liked(50).unwrap_err();
        assert!(err.to_string().contains("timed out"));
        assert!(matches!(err, Error::Catalog(_)));
    }

    #[test]
    fn playlist_loads_a_page_not_the_whole_library() {
        let yt = MockYt::default();
        Catalog::new(yt).playlist("PLabc", 80).unwrap();
    }

    #[test]
    fn album_keeps_browse_id_and_stamps_tracks() {
        let result = Catalog::new(MockYt::default())
            .album("MPREb_rooted")
            .unwrap();
        assert_eq!(result["id"], "MPREb_rooted");
        assert_eq!(result["playlistId"], "OLAK5uy_abc");
        assert_eq!(result["type"], "album");
        let track = &result["tracks"][0];
        assert_eq!(track["albumItem"]["id"], "MPREb_rooted");
        assert_eq!(track["albumItem"]["playlistId"], "OLAK5uy_abc");
        assert!(!get_text(&track["albumItem"], "imageUrl").is_empty());
        assert_eq!(track["album"], "Rooted Through Darkness");
    }

    #[test]
    fn playlist_keeps_requested_id_when_raw_omits_it() {
        let result = Catalog::new(MockYt::default())
            .playlist("VLPLQ6abc", 80)
            .unwrap();
        assert_eq!(result["id"], "VLPLQ6abc");
        assert_eq!(result["playlistId"], "VLPLQ6abc");
        assert_eq!(result["type"], "playlist");
        assert_eq!(result["tracks"][0]["videoId"], "abcdefghijk");
    }
}
