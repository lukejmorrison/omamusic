use crate::catalog::YtmClient;
use crate::error::{Error, Result};
use crate::json_util::{as_text, collect_thumbnails, find_first_str, runs_text, walk};
use crate::protocol::redact;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";
pub const YTM_DOMAIN: &str = "https://music.youtube.com";
pub const YTM_BASE_API: &str = "https://music.youtube.com/youtubei/v1/";
const API_KEY_ENV: &str = "YTM_API_KEY";

#[derive(Clone)]
pub struct Innertube {
    client: Client,
    headers: HeaderMap,
    visitor_id: String,
    signed_in: bool,
    api_key: String,
}

impl Innertube {
    pub fn unauthenticated() -> Result<Self> {
        let client = build_client()?;
        let mut api = Self {
            client,
            headers: default_headers(),
            visitor_id: String::new(),
            signed_in: false,
            api_key: String::new(),
        };
        api.hydrate()?;
        Ok(api)
    }

    pub fn from_browser_json(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let data: Value = serde_json::from_str(&text)?;
        Self::from_headers_json(&data)
    }

    pub fn from_oauth_token(access_token: &str) -> Result<Self> {
        Self::from_headers_json(&json!({
            "authorization": format!("Bearer {access_token}"),
            "origin": YTM_DOMAIN,
            "x-origin": YTM_DOMAIN,
        }))
    }

    pub fn from_headers_json(headers: &Value) -> Result<Self> {
        let client = build_client()?;
        let mut map = default_headers();
        if let Some(obj) = headers.as_object() {
            for (key, value) in obj {
                if let Some(text) = value.as_str() {
                    if let (Ok(name), Ok(val)) = (
                        HeaderName::from_bytes(key.as_bytes()),
                        HeaderValue::from_str(text),
                    ) {
                        map.insert(name, val);
                    }
                }
            }
        }
        let mut api = Self {
            client,
            headers: map,
            visitor_id: String::new(),
            signed_in: true,
            api_key: String::new(),
        };
        api.hydrate()?;
        Ok(api)
    }

    pub fn signed_in(&self) -> bool {
        self.signed_in
    }

    fn hydrate(&mut self) -> Result<()> {
        let (visitor_id, api_key) = hydrate_from_home(self.signed_in, self.fetch_home_html())?;
        self.visitor_id = visitor_id;
        self.api_key = api_key;
        Ok(())
    }

    fn fetch_home_html(&self) -> Result<String> {
        let response = self
            .client
            .get(YTM_DOMAIN)
            .headers(default_headers())
            .send()
            .map_err(|e| Error::catalog(e.to_string()))?;
        Ok(response.text().unwrap_or_default())
    }

    fn context(&self) -> Value {
        let day = chrono_yyyymmdd();
        json!({
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": format!("1.{day}.01.00"),
            },
            "user": {}
        })
    }

    pub fn send(&self, endpoint: &str, mut body: Value) -> Result<Value> {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("context".into(), self.context());
        }
        let mut headers = self.headers.clone();
        if !self.visitor_id.is_empty() && headers.get("X-Goog-Visitor-Id").is_none() {
            if let Ok(val) = HeaderValue::from_str(&self.visitor_id) {
                headers.insert("X-Goog-Visitor-Id", val);
            }
        }
        let url = innertube_url(endpoint, self.signed_in, &self.api_key);
        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .map_err(|e| Error::catalog(e.to_string()))?;
        let status = response.status();
        let text = response.text().unwrap_or_default();
        let json: Value = serde_json::from_str(&text).unwrap_or(json!({}));
        if status.as_u16() >= 400 {
            let reason = json
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or(status.canonical_reason().unwrap_or("error"));
            let message = format!("Server returned HTTP {status}: {reason}.");
            return Err(Error::catalog(redact(&message)));
        }
        Ok(json)
    }

    fn browse(&self, browse_id: &str) -> Result<Value> {
        self.send("browse", json!({"browseId": browse_id}))
    }
}

impl YtmClient for Innertube {
    fn get_account_info(&self) -> Result<Value> {
        let response = self.send("account/account_menu", json!({}))?;
        let mut name = String::new();
        walk(&response, &mut |map| {
            if name.is_empty() {
                if let Some(header) = map.get("activeAccountHeaderRenderer") {
                    name = runs_text(&header.get("accountName").cloned().unwrap_or(Value::Null));
                }
            }
        });
        Ok(json!({"accountName": name, "name": name}))
    }

    fn get_home(&self, limit: usize) -> Result<Vec<Value>> {
        let response = self.browse("FEmusic_home")?;
        Ok(extract_shelves(&response, limit))
    }

    fn get_history(&self) -> Result<Vec<Value>> {
        let response = self.browse("FEmusic_history")?;
        Ok(extract_tracks(&response, 80))
    }

    fn get_liked_songs(&self, limit: usize) -> Result<Value> {
        self.get_playlist("LM", limit)
    }

    fn get_library_playlists(&self, limit: usize) -> Result<Vec<Value>> {
        let response = self.browse("FEmusic_liked_playlists")?;
        Ok(extract_playlists(&response, limit))
    }

    fn get_library_songs(&self, limit: usize) -> Result<Vec<Value>> {
        let response = self.browse("FEmusic_liked_videos")?;
        Ok(extract_tracks(&response, limit))
    }

    fn get_library_albums(&self, limit: usize) -> Result<Vec<Value>> {
        let response = self.browse("FEmusic_liked_albums")?;
        Ok(extract_albums(&response, limit))
    }

    fn get_library_artists(&self, limit: usize) -> Result<Vec<Value>> {
        let response = self.browse("FEmusic_library_corpus_track_artists")?;
        Ok(extract_artists(&response, limit))
    }

    fn search(&self, query: &str, filter: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        let mut body = json!({"query": query});
        if let Some(filter) = filter {
            if let Some(params) = search_params(filter) {
                body["params"] = json!(params);
            }
        }
        let response = self.send("search", body)?;
        let mut items = extract_search_results(&response, filter, limit);
        items.truncate(limit);
        Ok(items)
    }

    fn get_playlist(&self, playlist_id: &str, limit: usize) -> Result<Value> {
        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id.to_string()
        } else {
            format!("VL{playlist_id}")
        };
        let response = self.browse(&browse_id)?;
        let mut header = extract_header(&response, "playlist", playlist_id);
        let tracks = extract_tracks(&response, limit);
        header["tracks"] = json!(tracks);
        header["title"] = header.get("name").cloned().unwrap_or(json!("Playlist"));
        Ok(header)
    }

    fn get_album(&self, album_id: &str) -> Result<Value> {
        let response = self.browse(album_id)?;
        let mut header = extract_header(&response, "album", album_id);
        let tracks = extract_tracks(&response, 0);
        header["tracks"] = json!(tracks);
        header["title"] = header.get("name").cloned().unwrap_or(json!("Album"));
        if header.get("audioPlaylistId").is_none() {
            if let Some(pid) = header.get("playlistId") {
                header["audioPlaylistId"] = pid.clone();
            }
        }
        Ok(header)
    }

    fn get_artist(&self, artist_id: &str) -> Result<Value> {
        let response = self.browse(artist_id)?;
        let mut header = extract_header(&response, "artist", artist_id);
        let tracks = extract_tracks(&response, 20);
        let albums = extract_albums(&response, 20);
        let singles = extract_albums(&response, 12);
        header["name"] = header.get("name").cloned().unwrap_or(json!("Artist"));
        header["songs"] = json!({"results": tracks});
        header["albums"] = json!({"results": albums});
        header["singles"] = json!({"results": singles});
        header["radioId"] = json!(find_first_str(&response, "playlistId"));
        Ok(header)
    }

    fn get_watch_playlist(&self, video_id: &str, limit: usize) -> Result<Value> {
        let playlist_id = format!("RDAMVM{video_id}");
        let body = json!({
            "enablePersistentPlaylistPanel": true,
            "isAudioOnly": true,
            "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            "videoId": video_id,
            "playlistId": playlist_id,
            "watchEndpointMusicSupportedConfigs": {
                "watchEndpointMusicConfig": {
                    "hasPersistentPlaylistPanel": true,
                    "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
                }
            }
        });
        let response = self.send("next", body)?;
        Ok(json!({"tracks": extract_tracks(&response, limit)}))
    }

    fn rate_song(&self, video_id: &str, rating: &str) -> Result<()> {
        let endpoint = like_endpoint(rating);
        self.send(endpoint, json!({"target": {"videoId": video_id}}))?;
        Ok(())
    }

    fn rate_playlist(&self, playlist_id: &str, rating: &str) -> Result<()> {
        let endpoint = like_endpoint(rating);
        self.send(endpoint, json!({"target": {"playlistId": playlist_id}}))?;
        Ok(())
    }

    fn create_playlist(&self, name: &str) -> Result<String> {
        let response = self.send(
            "playlist/create",
            json!({
                "title": name,
                "description": "",
                "privacyStatus": "PRIVATE",
            }),
        )?;
        response
            .get("playlistId")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .ok_or_else(|| Error::catalog("Could not create playlist"))
    }

    fn add_playlist_items(&self, playlist_id: &str, video_id: &str) -> Result<()> {
        let id = playlist_id.strip_prefix("VL").unwrap_or(playlist_id);
        self.send(
            "browse/edit_playlist",
            json!({
                "playlistId": id,
                "actions": [{
                    "action": "ACTION_ADD_VIDEO",
                    "addedVideoId": video_id,
                }],
            }),
        )?;
        Ok(())
    }
}

fn like_endpoint(rating: &str) -> &'static str {
    match rating {
        "LIKE" => "like/like",
        "DISLIKE" => "like/dislike",
        _ => "like/removelike",
    }
}

fn search_params(filter: &str) -> Option<&'static str> {
    match filter {
        "songs" => Some("EgWKAQIIAWoMEA4QChADEAQQCRAF"),
        "albums" => Some("EgWKAQIYAWoMEA4QChADEAQQCRAF"),
        "artists" => Some("EgWKAQIgAWoMEA4QChADEAQQCRAF"),
        "playlists" => Some("Eg-KAQwIABAAGAAgACgBMABqChAEEAMQCRAFEAo%3D"),
        "videos" => Some("EgWKAQIQAWoMEA4QChADEAQQCRAF"),
        _ => None,
    }
}

fn innertube_url(endpoint: &str, signed_in: bool, api_key: &str) -> String {
    let mut url = format!("{YTM_BASE_API}{endpoint}?alt=json");
    if signed_in {
        url.push_str("&key=");
        url.push_str(api_key);
    }
    url
}

fn env_api_key() -> Option<String> {
    match std::env::var(API_KEY_ENV) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

fn parse_ytcfg(html: &str) -> Option<Value> {
    let start = html.find("ytcfg.set(")?;
    let slice = &html[start + 10..];
    let end = slice.find(");")?;
    serde_json::from_str(&slice[..end]).ok()
}

fn parse_quoted_field(html: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\"");
    let rest = html.split(&needle).nth(1)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_innertube_api_key(html: &str) -> Option<String> {
    if let Some(cfg) = parse_ytcfg(html) {
        if let Some(key) = cfg.get("INNERTUBE_API_KEY").and_then(Value::as_str) {
            let key = key.trim();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
    }
    parse_quoted_field(html, "INNERTUBE_API_KEY")
}

fn missing_api_key_error() -> Error {
    Error::catalog(format!(
        "Could not resolve YouTube Music InnerTube client key. Set {API_KEY_ENV} or ensure {YTM_DOMAIN} is reachable."
    ))
}

fn resolve_api_key(html: Option<&str>) -> Result<String> {
    if let Some(key) = env_api_key() {
        return Ok(key);
    }
    if let Some(html) = html {
        if let Some(key) = parse_innertube_api_key(html) {
            return Ok(key);
        }
    }
    Err(missing_api_key_error())
}

fn parse_visitor_id(html: &str) -> String {
    parse_ytcfg(html)
        .and_then(|cfg| {
            cfg.get("VISITOR_DATA")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Apply homepage HTML (or a fetch failure) to visitor id + cached key.
/// Guest URLs omit `&key=`, so a missing key is not fatal there. Signed-in
/// URLs need the key; a homepage fetch error is preserved in that case.
fn hydrate_from_home(signed_in: bool, fetch: Result<String>) -> Result<(String, String)> {
    match fetch {
        Ok(html) => {
            let visitor_id = parse_visitor_id(&html);
            match resolve_api_key(Some(&html)) {
                Ok(key) => Ok((visitor_id, key)),
                Err(_) if !signed_in => Ok((visitor_id, String::new())),
                Err(missing) => Err(missing),
            }
        }
        Err(err) => match resolve_api_key(None) {
            Ok(key) => Ok((String::new(), key)),
            Err(_) if !signed_in => Ok((String::new(), String::new())),
            Err(missing) => Err(Error::catalog(format!(
                "{missing} Homepage fetch failed: {err}"
            ))),
        },
    }
}

fn build_client() -> Result<Client> {
    Client::builder()
        .gzip(true)
        .timeout(std::time::Duration::from_secs(30))
        .cookie_store(true)
        .build()
        .map_err(|e| Error::catalog(e.to_string()))
}

fn default_headers() -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert("user-agent", HeaderValue::from_static(USER_AGENT));
    map.insert("accept", HeaderValue::from_static("*/*"));
    map.insert("content-type", HeaderValue::from_static("application/json"));
    map.insert("origin", HeaderValue::from_static(YTM_DOMAIN));
    map
}

fn chrono_yyyymmdd() -> String {
    chrono::Utc::now().format("%Y%m%d").to_string()
}

fn extract_shelves(root: &Value, limit: usize) -> Vec<Value> {
    let mut shelves = Vec::new();
    walk(root, &mut |map| {
        if shelves.len() >= limit {
            return;
        }
        if let Some(car) = map.get("musicCarouselShelfRenderer") {
            if let Some(shelf) = shelf_from_renderer(car) {
                shelves.push(shelf);
            }
        } else if let Some(shelf) = map.get("musicShelfRenderer") {
            if let Some(parsed) = shelf_from_renderer(shelf) {
                shelves.push(parsed);
            }
        }
    });
    shelves.truncate(limit);
    shelves
}

fn shelf_from_renderer(renderer: &Value) -> Option<Value> {
    let title = header_title(renderer);
    let mut contents = Vec::new();
    if let Some(Value::Array(items)) = renderer.get("contents") {
        for item in items {
            if let Some(parsed) = parse_renderer(item) {
                contents.push(parsed);
            }
        }
    }
    if contents.is_empty() {
        return None;
    }
    Some(json!({
        "title": title,
        "contents": contents,
    }))
}

fn header_title(renderer: &Value) -> String {
    let mut title = String::new();
    walk(renderer.get("header").unwrap_or(&Value::Null), &mut |map| {
        if title.is_empty() {
            if let Some(t) = map.get("title") {
                title = runs_text(t);
            }
        }
    });
    if title.is_empty() {
        title = runs_text(renderer.get("title").unwrap_or(&Value::Null));
    }
    title
}

fn extract_tracks(root: &Value, limit: usize) -> Vec<Value> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    walk(root, &mut |map| {
        if limit > 0 && items.len() >= limit {
            return;
        }
        for key in [
            "playlistPanelVideoRenderer",
            "musicResponsiveListItemRenderer",
            "musicTwoRowItemRenderer",
        ] {
            if let Some(renderer) = map.get(key) {
                if let Some(parsed) = parse_named_renderer(key, renderer) {
                    if let Some(vid) = parsed.get("videoId").and_then(Value::as_str) {
                        if !vid.is_empty() && seen.insert(vid.to_string()) {
                            items.push(parsed);
                        }
                    }
                }
            }
        }
    });
    if limit > 0 {
        items.truncate(limit);
    }
    items
}

fn extract_playlists(root: &Value, limit: usize) -> Vec<Value> {
    collect_typed(root, "playlist", limit)
}

fn extract_albums(root: &Value, limit: usize) -> Vec<Value> {
    collect_typed(root, "album", limit)
}

fn extract_artists(root: &Value, limit: usize) -> Vec<Value> {
    collect_typed(root, "artist", limit)
}

fn collect_typed(root: &Value, result_type: &str, limit: usize) -> Vec<Value> {
    let mut items = Vec::new();
    walk(root, &mut |map| {
        if limit > 0 && items.len() >= limit {
            return;
        }
        for key in ["musicTwoRowItemRenderer", "musicResponsiveListItemRenderer"] {
            if let Some(renderer) = map.get(key) {
                if let Some(mut parsed) = parse_named_renderer(key, renderer) {
                    let parsed_type = parsed
                        .get("resultType")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if parsed_type == result_type
                        || (result_type == "playlist" && parsed.get("playlistId").is_some())
                    {
                        parsed["resultType"] = json!(result_type);
                        items.push(parsed);
                    }
                }
            }
        }
    });
    if limit > 0 {
        items.truncate(limit);
    }
    items
}

fn extract_search_results(root: &Value, filter: Option<&str>, limit: usize) -> Vec<Value> {
    let mut items = Vec::new();
    walk(root, &mut |map| {
        if items.len() >= limit.max(24) {
            return;
        }
        if let Some(renderer) = map.get("musicResponsiveListItemRenderer") {
            if let Some(mut parsed) = parse_mrlir(renderer) {
                if let Some(filter) = filter {
                    let want = match filter {
                        "songs" | "videos" => "song",
                        "albums" => "album",
                        "artists" => "artist",
                        "playlists" => "playlist",
                        _ => "",
                    };
                    let got = parsed
                        .get("resultType")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !want.is_empty()
                        && got != want
                        && !(want == "song" && parsed.get("videoId").is_some())
                    {
                        if want == "song" {
                            parsed["resultType"] = json!("song");
                        } else {
                            return;
                        }
                    }
                }
                items.push(parsed);
            }
        }
    });
    items
}

fn extract_header(root: &Value, item_type: &str, fallback_id: &str) -> Value {
    let mut title = String::new();
    let mut subtitle = String::new();
    let mut browse_id = fallback_id.to_string();
    let mut playlist_id = String::new();
    walk(root, &mut |map| {
        if title.is_empty() {
            if let Some(header) = map
                .get("musicResponsiveHeaderRenderer")
                .or_else(|| map.get("musicDetailHeaderRenderer"))
            {
                title = runs_text(header.get("title").unwrap_or(&Value::Null));
                subtitle = runs_text(header.get("subtitle").unwrap_or(&Value::Null));
            }
        }
        if playlist_id.is_empty() {
            if let Some(id) = map.get("playlistId").and_then(Value::as_str) {
                if id.starts_with("OL") || id.starts_with("PL") || id == "LM" {
                    playlist_id = id.to_string();
                }
            }
        }
        if let Some(id) = map.get("browseId").and_then(Value::as_str) {
            if item_type == "album" && id.starts_with("MPRE") {
                browse_id = id.to_string();
            }
        }
    });
    if title.is_empty() {
        title = find_first_str(root, "title");
    }
    let thumbs = collect_thumbnails(root);
    json!({
        "title": title,
        "name": title,
        "subtitle": subtitle,
        "browseId": browse_id,
        "id": browse_id,
        "playlistId": playlist_id,
        "audioPlaylistId": playlist_id,
        "thumbnails": thumbs,
        "resultType": item_type,
        "type": item_type,
    })
}

fn parse_renderer(item: &Value) -> Option<Value> {
    if let Some(r) = item.get("musicResponsiveListItemRenderer") {
        return parse_mrlir(r);
    }
    if let Some(r) = item.get("musicTwoRowItemRenderer") {
        return parse_two_row(r);
    }
    if let Some(r) = item.get("playlistPanelVideoRenderer") {
        return parse_panel_video(r);
    }
    None
}

fn parse_named_renderer(key: &str, renderer: &Value) -> Option<Value> {
    match key {
        "musicResponsiveListItemRenderer" => parse_mrlir(renderer),
        "musicTwoRowItemRenderer" => parse_two_row(renderer),
        "playlistPanelVideoRenderer" => parse_panel_video(renderer),
        _ => None,
    }
}

fn page_type(value: &Value) -> String {
    find_first_str(value, "pageType")
}

fn parse_mrlir(renderer: &Value) -> Option<Value> {
    let video_id = find_first_str(renderer, "videoId");
    let browse_id = find_first_str(renderer, "browseId");
    let playlist_id = overlay_playlist_id(renderer);
    let title = flex_text(renderer, 0);
    if title.is_empty() {
        return None;
    }
    let subtitle = flex_text(renderer, 1);
    let thumbs = collect_thumbnails(renderer);
    let page = page_type(renderer);
    let mut result_type = if !video_id.is_empty() {
        "song"
    } else if page.contains("ALBUM") || browse_id.starts_with("MPRE") {
        "album"
    } else if page.contains("ARTIST") || browse_id.starts_with("UC") {
        "artist"
    } else if page.contains("PLAYLIST") || !playlist_id.is_empty() || browse_id.starts_with("PL") {
        "playlist"
    } else {
        "song"
    };
    if result_type == "song" && video_id.is_empty() && !browse_id.is_empty() {
        result_type = if browse_id.starts_with("MPRE") {
            "album"
        } else if browse_id.starts_with("UC") {
            "artist"
        } else {
            "playlist"
        };
    }
    let artists = artists_from_subtitle(&subtitle, renderer);
    let mut item = json!({
        "title": title,
        "videoId": video_id,
        "browseId": browse_id,
        "playlistId": playlist_id,
        "thumbnails": thumbs,
        "resultType": result_type,
        "artists": artists,
        "likeStatus": find_first_str(renderer, "likeStatus"),
        "setVideoId": find_first_str(renderer, "setVideoId"),
    });
    if let Some(duration) = duration_from_flex(&subtitle) {
        item["duration"] = json!(duration);
    }
    if result_type == "album" {
        item["name"] = json!(title);
    }
    Some(item)
}

fn parse_two_row(renderer: &Value) -> Option<Value> {
    let title = runs_text(renderer.get("title").unwrap_or(&Value::Null));
    if title.is_empty() {
        return None;
    }
    let subtitle = runs_text(renderer.get("subtitle").unwrap_or(&Value::Null));
    let browse_id = find_first_str(renderer, "browseId");
    let video_id = find_first_str(renderer, "videoId");
    let playlist_id = overlay_playlist_id(renderer);
    let page = page_type(renderer);
    let result_type = if page.contains("ALBUM") || browse_id.starts_with("MPRE") {
        "album"
    } else if page.contains("ARTIST") || browse_id.starts_with("UC") {
        "artist"
    } else if page.contains("PLAYLIST") || !playlist_id.is_empty() {
        "playlist"
    } else if !video_id.is_empty() {
        "song"
    } else {
        "album"
    };
    Some(json!({
        "title": title,
        "name": title,
        "subtitle": subtitle,
        "browseId": browse_id,
        "videoId": video_id,
        "playlistId": playlist_id,
        "audioPlaylistId": playlist_id,
        "thumbnails": collect_thumbnails(renderer),
        "resultType": result_type,
        "artists": artists_from_subtitle(&subtitle, renderer),
        "subscribers": if result_type == "artist" { subtitle } else { String::new() },
    }))
}

fn parse_panel_video(renderer: &Value) -> Option<Value> {
    let video_id = find_first_str(renderer, "videoId");
    let title = runs_text(renderer.get("title").unwrap_or(&Value::Null));
    if video_id.is_empty() || title.is_empty() {
        return None;
    }
    let length = runs_text(renderer.get("lengthText").unwrap_or(&Value::Null));
    Some(json!({
        "title": title,
        "videoId": video_id,
        "duration": length,
        "thumbnails": collect_thumbnails(renderer),
        "resultType": "song",
        "artists": artists_from_runs(renderer.get("longBylineText").unwrap_or(&Value::Null)),
        "likeStatus": find_first_str(renderer, "likeStatus"),
        "album": album_from_runs(renderer.get("longBylineText").unwrap_or(&Value::Null)),
    }))
}

fn flex_text(renderer: &Value, index: usize) -> String {
    renderer
        .get("flexColumns")
        .and_then(Value::as_array)
        .and_then(|cols| cols.get(index))
        .map(|col| {
            let text = col
                .pointer("/musicResponsiveListItemFlexColumnRenderer/text")
                .cloned()
                .unwrap_or(Value::Null);
            runs_text(&text)
        })
        .unwrap_or_default()
}

fn duration_from_flex(subtitle: &str) -> Option<String> {
    subtitle.split('•').last().map(str::trim).and_then(|part| {
        if part.contains(':') {
            Some(part.to_string())
        } else {
            None
        }
    })
}

fn overlay_playlist_id(renderer: &Value) -> String {
    let mut playlist_id = find_first_str(renderer, "playlistId");
    if playlist_id.is_empty() {
        walk(renderer, &mut |map| {
            if playlist_id.is_empty() {
                if let Some(id) = map.get("playlistId").and_then(Value::as_str) {
                    playlist_id = id.to_string();
                }
            }
        });
    }
    playlist_id
}

fn artists_from_subtitle(subtitle: &str, renderer: &Value) -> Value {
    let from_runs = artists_from_runs(renderer);
    if from_runs.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
        return from_runs;
    }
    let name = subtitle.split('•').next().unwrap_or("").trim();
    if name.is_empty() {
        json!([])
    } else {
        json!([{"name": name, "id": ""}])
    }
}

fn artists_from_runs(value: &Value) -> Value {
    let mut artists = Vec::new();
    walk(value, &mut |map| {
        if let Some(runs) = map.get("runs").and_then(Value::as_array) {
            for run in runs {
                let name = as_text(&run.get("text").cloned().unwrap_or(Value::Null));
                if name.is_empty() || name == "•" || name.contains(':') {
                    continue;
                }
                let id = run
                    .pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let page = run
                    .pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if page.contains("ARTIST") || id.starts_with("UC") {
                    artists.push(json!({"name": name, "id": id}));
                }
            }
        }
    });
    json!(artists)
}

fn album_from_runs(value: &Value) -> Value {
    let mut album = json!({});
    walk(value, &mut |map| {
        if let Some(runs) = map.get("runs").and_then(Value::as_array) {
            for run in runs {
                let id = run
                    .pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if id.starts_with("MPRE") {
                    album = json!({
                        "name": as_text(&run.get("text").cloned().unwrap_or(Value::Null)),
                        "id": id,
                        "browseId": id,
                    });
                }
            }
        }
    });
    album
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        previous: Option<String>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn lock() -> MutexGuard<'static, ()> {
            ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner())
        }

        fn set(value: Option<&str>) -> Self {
            let lock = Self::lock();
            let previous = std::env::var(API_KEY_ENV).ok();
            match value {
                Some(value) => std::env::set_var(API_KEY_ENV, value),
                None => std::env::remove_var(API_KEY_ENV),
            }
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(API_KEY_ENV, value),
                None => std::env::remove_var(API_KEY_ENV),
            }
        }
    }

    #[test]
    fn parse_key_from_ytcfg_json() {
        let html = r#"<script>ytcfg.set({"INNERTUBE_API_KEY":"ytm-test-client-key","VISITOR_DATA":"Cgtabc"});</script>"#;
        assert_eq!(
            parse_innertube_api_key(html).as_deref(),
            Some("ytm-test-client-key")
        );
        assert_eq!(
            parse_ytcfg(html)
                .and_then(|cfg| cfg
                    .get("VISITOR_DATA")
                    .and_then(Value::as_str)
                    .map(str::to_string))
                .as_deref(),
            Some("Cgtabc")
        );
    }

    #[test]
    fn parse_key_from_quoted_field_when_ytcfg_is_unparseable() {
        let html = r#"var cfg = {"INNERTUBE_API_KEY":"quoted-test-key"};"#;
        assert_eq!(
            parse_innertube_api_key(html).as_deref(),
            Some("quoted-test-key")
        );
    }

    #[test]
    fn parse_key_ignores_empty_values() {
        assert_eq!(
            parse_innertube_api_key(r#"ytcfg.set({"INNERTUBE_API_KEY":""});"#),
            None
        );
        assert_eq!(parse_innertube_api_key("<html></html>"), None);
    }

    #[test]
    fn resolve_prefers_env_over_page() {
        let _guard = EnvGuard::set(Some("from-env-test-key"));
        let html = r#"ytcfg.set({"INNERTUBE_API_KEY":"from-page-key"});"#;
        assert_eq!(resolve_api_key(Some(html)).unwrap(), "from-env-test-key");
    }

    #[test]
    fn resolve_uses_page_when_env_unset() {
        let _guard = EnvGuard::set(None);
        let html = r#"ytcfg.set({"INNERTUBE_API_KEY":"from-page-key"});"#;
        assert_eq!(resolve_api_key(Some(html)).unwrap(), "from-page-key");
    }

    #[test]
    fn resolve_treats_blank_env_as_unset() {
        let _guard = EnvGuard::set(Some("   "));
        let html = r#"ytcfg.set({"INNERTUBE_API_KEY":"from-page-key"});"#;
        assert_eq!(resolve_api_key(Some(html)).unwrap(), "from-page-key");
    }

    #[test]
    fn resolve_fails_clearly_when_missing() {
        let _guard = EnvGuard::set(None);
        let err = resolve_api_key(Some("<html></html>")).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(API_KEY_ENV), "{message}");
        assert!(message.contains(YTM_DOMAIN), "{message}");
    }

    #[test]
    fn signed_in_url_uses_cached_key() {
        let url = innertube_url("browse", true, "cached-test-key");
        assert_eq!(
            url,
            "https://music.youtube.com/youtubei/v1/browse?alt=json&key=cached-test-key"
        );
    }

    #[test]
    fn guest_url_omits_key() {
        let url = innertube_url("search", false, "cached-test-key");
        assert_eq!(url, "https://music.youtube.com/youtubei/v1/search?alt=json");
        assert!(!url.contains("key="));
    }

    #[test]
    fn guest_hydrate_allows_missing_key() {
        let _guard = EnvGuard::set(None);
        let (visitor, key) = hydrate_from_home(false, Ok("<html></html>".into())).unwrap();
        assert_eq!(visitor, "");
        assert_eq!(key, "");
    }

    #[test]
    fn guest_hydrate_survives_homepage_fetch_error() {
        let _guard = EnvGuard::set(None);
        let (visitor, key) =
            hydrate_from_home(false, Err(Error::catalog("connection refused"))).unwrap();
        assert_eq!(visitor, "");
        assert_eq!(key, "");
    }

    #[test]
    fn guest_hydrate_still_caches_page_key() {
        let _guard = EnvGuard::set(None);
        let html = r#"ytcfg.set({"INNERTUBE_API_KEY":"from-page-key","VISITOR_DATA":"vid"});"#;
        let (visitor, key) = hydrate_from_home(false, Ok(html.into())).unwrap();
        assert_eq!(visitor, "vid");
        assert_eq!(key, "from-page-key");
    }

    #[test]
    fn signed_in_hydrate_wraps_homepage_fetch_error() {
        let _guard = EnvGuard::set(None);
        let err = hydrate_from_home(true, Err(Error::catalog("connection refused"))).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("connection refused"), "{message}");
        assert!(message.contains(API_KEY_ENV), "{message}");
    }

    #[test]
    fn signed_in_hydrate_uses_env_when_homepage_fails() {
        let _guard = EnvGuard::set(Some("from-env-test-key"));
        let (_, key) = hydrate_from_home(true, Err(Error::catalog("connection refused"))).unwrap();
        assert_eq!(key, "from-env-test-key");
    }
}
