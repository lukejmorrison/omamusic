use crate::error::{Error, Result};
use crate::paths::{chmod, which, AppPaths};
use crate::protocol::redact;
use aes::Aes128;
use cbc::Decryptor;
use cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use pbkdf2::pbkdf2_hmac;
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_AUTH_NAME: &str = "browser.json";
const CHROME_KEY_SALT: &[u8] = b"saltysalt";
const CHROME_IV: &[u8] = b"                ";
const CHROME_PBKDF2_ROUNDS: u32 = 1;
const CHROME_KEY_LEN: usize = 16;
const CHROME_HASH_PREFIX_LEN: usize = 32;

type Aes128CbcDec = Decryptor<Aes128>;

#[derive(Clone, Debug)]
pub struct CookieDatabase {
    pub keyring: String,
    pub browser: String,
    pub profile: String,
    pub path: PathBuf,
}

pub fn default_auth_path(paths: &AppPaths) -> PathBuf {
    paths.auth_path()
}

pub fn resolve_auth_path(paths: &AppPaths, explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        if path.is_file() {
            return path.to_path_buf();
        }
    }
    let default = paths.auth_path();
    if default.is_file() && default.metadata().map(|m| m.len() > 2).unwrap_or(false) {
        return default;
    }
    for candidate in paths.legacy_auth_candidates() {
        if candidate.is_file() && candidate.metadata().map(|m| m.len() > 2).unwrap_or(false) {
            return candidate;
        }
    }
    default
}

pub fn auth_available(path: &Path) -> bool {
    path.is_file() && path.metadata().map(|m| m.len() > 2).unwrap_or(false)
}

pub fn load_headers(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)?;
    let data: Value = serde_json::from_str(&text)?;
    if !data.is_object() {
        return Err(Error::invalid("browser.json must be a JSON object"));
    }
    Ok(data)
}

pub fn cookie_header(headers: &Value) -> String {
    let Some(obj) = headers.as_object() else {
        return String::new();
    };
    for (key, value) in obj {
        if key.eq_ignore_ascii_case("cookie") {
            return value.as_str().unwrap_or("").to_string();
        }
    }
    String::new()
}

pub fn parse_cookie_header(header: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for part in header.split(';') {
        let item = part.trim();
        if item.is_empty() || !item.contains('=') {
            continue;
        }
        let (name, value) = item.split_once('=').unwrap();
        let name = name.trim();
        if !name.is_empty() {
            pairs.push((name.to_string(), value.trim().to_string()));
        }
    }
    pairs
}

pub fn sapisid_value(header: &str) -> String {
    parse_cookie_header(header)
        .into_iter()
        .find(|(name, _)| name == "__Secure-3PAPISID")
        .map(|(_, value)| value)
        .unwrap_or_default()
}

pub fn sapisidhash(header: &str, origin: &str) -> String {
    let sapisid = sapisid_value(header);
    if sapisid.is_empty() {
        return String::new();
    }
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let digest = Sha1::digest(format!("{unix} {sapisid} {origin}").as_bytes());
    format!("SAPISIDHASH {unix}_{}", hex::encode(digest))
}

pub fn headers_raw_from_cookies(pairs: &[(String, String)], authuser: &str) -> String {
    let header = pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    let origin = "https://music.youtube.com";
    let authorization = sapisidhash(&header, origin);
    let mut lines = vec![
        format!("cookie: {header}"),
        format!("x-goog-authuser: {authuser}"),
        format!("origin: {origin}"),
        format!("x-origin: {origin}"),
    ];
    if !authorization.is_empty() {
        lines.push(format!("authorization: {authorization}"));
    }
    lines.join("\n") + "\n"
}

pub fn parse_headers_raw(headers_raw: &str) -> Result<Value> {
    let mut user_headers: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut chrome_remembered_key = String::new();
    for content in headers_raw.split('\n') {
        if content.starts_with(':') {
            continue;
        }
        let mut header = content.splitn(2, ": ");
        let key = header.next().unwrap_or("");
        let value = header.next();
        if key.ends_with(':') && value.is_none() {
            chrome_remembered_key = content.replace(':', "");
            continue;
        }
        if let Some(value) = value {
            user_headers.insert(key.to_ascii_lowercase(), json!(value));
            chrome_remembered_key.clear();
        } else if !chrome_remembered_key.is_empty() {
            user_headers.insert(chrome_remembered_key.clone(), json!(key));
            if !key.ends_with(':') && (chrome_remembered_key != "Decoded" || key == "}") {
                chrome_remembered_key.clear();
            }
        } else {
            chrome_remembered_key = key.to_string();
        }
    }
    let keys: Vec<String> = user_headers
        .keys()
        .map(|k| k.to_ascii_lowercase())
        .collect();
    let mut missing = Vec::new();
    for required in ["cookie", "x-goog-authuser"] {
        if !keys.iter().any(|k| k == required) {
            missing.push(required);
        }
    }
    if !missing.is_empty() {
        return Err(Error::auth(format!(
            "The following entries are missing in your headers: {}. Please try a different request (such as /browse) and make sure you are logged in.",
            missing.join(", ")
        )));
    }
    for key in user_headers.clone().keys() {
        if key.starts_with("sec")
            || matches!(key.as_str(), "host" | "content-length" | "accept-encoding")
        {
            user_headers.remove(key);
        }
    }
    user_headers.insert("user-agent".into(), json!(crate::innertube::USER_AGENT));
    user_headers.insert("accept".into(), json!("*/*"));
    user_headers.insert("content-type".into(), json!("application/json"));
    user_headers.insert(
        "origin".into(),
        json!(user_headers
            .get("origin")
            .and_then(Value::as_str)
            .unwrap_or("https://music.youtube.com")),
    );
    Ok(Value::Object(user_headers))
}

pub fn save_headers(headers_raw: &str, dest: &Path) -> Result<PathBuf> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
        chmod(parent, 0o700);
    }
    let mut headers = parse_headers_raw(headers_raw)?;
    write_headers(&mut headers, dest)?;
    refresh_browser_authorization(dest)?;
    Ok(dest.to_path_buf())
}

fn write_headers(headers: &mut Value, dest: &Path) -> Result<()> {
    let text = serde_json::to_string_pretty(headers)?;
    fs::write(dest, text)?;
    chmod(dest, 0o600);
    Ok(())
}

pub fn refresh_browser_authorization(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let mut headers = load_headers(path)?;
    let header = cookie_header(&headers);
    let mut origin = headers
        .get("origin")
        .or_else(|| headers.get("x-origin"))
        .and_then(Value::as_str)
        .unwrap_or("https://music.youtube.com")
        .to_string();
    if origin.is_empty() {
        origin = "https://music.youtube.com".into();
    }
    let authorization = sapisidhash(&header, &origin);
    if authorization.is_empty() {
        return Ok(());
    }
    if let Some(obj) = headers.as_object_mut() {
        obj.insert("origin".into(), json!(origin.clone()));
        if !obj.contains_key("x-origin") {
            obj.insert("x-origin".into(), json!(origin));
        }
        let mut auth_key = "authorization".to_string();
        for key in obj.keys() {
            if key.eq_ignore_ascii_case("authorization") {
                auth_key = key.clone();
                break;
            }
        }
        obj.insert(auth_key, json!(authorization));
    }
    let text = serde_json::to_string_pretty(&headers)?;
    fs::write(path, text)?;
    chmod(path, 0o600);
    Ok(())
}

pub fn write_netscape_cookies(header: &str, dest: &Path) -> Result<PathBuf> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
        chmod(parent, 0o700);
    }
    let pairs = parse_cookie_header(header);
    let mut lines = vec![
        "# Netscape HTTP Cookie File".to_string(),
        "# Generated by Oma Music. Do not share this file.".to_string(),
    ];
    for (name, value) in pairs {
        let secure = if name.starts_with("__Secure-") || name.starts_with("__Host-") {
            "TRUE"
        } else {
            "FALSE"
        };
        lines.push(format!(
            ".youtube.com\tTRUE\t/\t{secure}\t0\t{name}\t{value}"
        ));
    }
    fs::write(dest, lines.join("\n") + "\n")?;
    chmod(dest, 0o600);
    Ok(dest.to_path_buf())
}

pub fn export_cookies(auth_path: &Path, dest: &Path) -> Option<PathBuf> {
    if !auth_available(auth_path) {
        return None;
    }
    let headers = load_headers(auth_path).ok()?;
    let header = cookie_header(&headers);
    if header.is_empty() {
        return None;
    }
    write_netscape_cookies(&header, dest).ok()
}

pub fn clear_auth(auth_path: &Path, cookies_path: &Path) {
    if auth_path.is_file() {
        let _ = fs::remove_file(auth_path);
    }
    if cookies_path.is_file() {
        let _ = fs::remove_file(cookies_path);
    }
}

pub fn iter_cookie_databases() -> Vec<CookieDatabase> {
    let home = crate::paths::home_dir();
    let roots = [
        ("chromium", "Chromium", home.join(".config/chromium")),
        ("chrome", "Chrome", home.join(".config/google-chrome")),
        (
            "brave",
            "Brave",
            home.join(".config/BraveSoftware/Brave-Browser"),
        ),
    ];
    let mut found = Vec::new();
    for (keyring, label, root) in roots {
        if !root.is_dir() {
            continue;
        }
        for profile in profile_dirs(&root) {
            for relative in ["Network/Cookies", "Cookies"] {
                let path = profile.join(relative);
                if path.is_file() {
                    found.push(CookieDatabase {
                        keyring: keyring.into(),
                        browser: label.into(),
                        profile: profile
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        path,
                    });
                    break;
                }
            }
        }
    }
    found
}

fn profile_dirs(root: &Path) -> Vec<PathBuf> {
    let mut profiles = Vec::new();
    let default = root.join("Default");
    if default.is_dir() {
        profiles.push(default);
    }
    let mut extras = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let child = entry.path();
            if !child.is_dir() || child.file_name().and_then(|n| n.to_str()) == Some("Default") {
                continue;
            }
            if child.join("Cookies").is_file() || child.join("Network/Cookies").is_file() {
                extras.push(child);
            }
        }
    }
    extras.sort();
    profiles.extend(extras);
    profiles
}

pub fn import_from_browser(
    dest: &Path,
    databases: Option<&[CookieDatabase]>,
    password_for: Option<&HashMap<String, Vec<u8>>>,
) -> Result<PathBuf> {
    let (pairs, _source) = extract_youtube_cookies(databases, password_for)?;
    let names: std::collections::HashSet<_> = pairs.iter().map(|(n, _)| n.as_str()).collect();
    if !names.contains("__Secure-3PAPISID") {
        return Err(Error::auth(
            "Chromium is not signed in to YouTube Music. Open music.youtube.com, sign in, then try again.",
        ));
    }
    save_headers(&headers_raw_from_cookies(&pairs, "0"), dest)
}

pub fn extract_youtube_cookies(
    databases: Option<&[CookieDatabase]>,
    password_for: Option<&HashMap<String, Vec<u8>>>,
) -> Result<(Vec<(String, String)>, CookieDatabase)> {
    let owned = if databases.is_none() {
        iter_cookie_databases()
    } else {
        vec![]
    };
    let candidates = databases.unwrap_or(&owned);
    if candidates.is_empty() {
        return Err(Error::auth(
            "No Chromium cookie database was found on this computer.",
        ));
    }
    let mut best: Option<((i32, i32, i32, i32), Vec<(String, String)>, CookieDatabase)> = None;
    let mut last_error = String::new();
    let mut passwords: HashMap<String, Vec<u8>> = password_for.cloned().unwrap_or_default();
    for database in candidates {
        let password = if let Some(p) = passwords.get(&database.keyring) {
            p.clone()
        } else {
            match os_crypt_password(&database.keyring) {
                Ok(p) => {
                    passwords.insert(database.keyring.clone(), p.clone());
                    p
                }
                Err(err) => {
                    last_error = err.to_string();
                    continue;
                }
            }
        };
        let pairs = match read_youtube_cookies(&database.path, &password) {
            Ok(p) => p,
            Err(err) => {
                last_error = err.to_string();
                continue;
            }
        };
        let names: std::collections::HashSet<_> = pairs.iter().map(|(n, _)| n.as_str()).collect();
        let rank = (
            if names.contains("__Secure-3PAPISID") {
                1
            } else {
                0
            },
            browser_rank(&database.keyring),
            if database.profile == "Default" { 1 } else { 0 },
            pairs.len() as i32,
        );
        if best.as_ref().map(|(r, _, _)| rank > *r).unwrap_or(true) {
            best = Some((rank, pairs, database.clone()));
        }
    }
    best.map(|(_, pairs, db)| (pairs, db)).ok_or_else(|| {
        Error::auth(if last_error.is_empty() {
            "Could not read Chromium cookies. Unlock the login keyring and try again.".into()
        } else {
            last_error
        })
    })
}

pub fn read_youtube_cookies(db_path: &Path, password: &[u8]) -> Result<Vec<(String, String)>> {
    let key = derive_os_crypt_key(password);
    let tmp = tempfile::tempdir().map_err(|e| Error::auth(e.to_string()))?;
    let copied = copy_sqlite(db_path, tmp.path())?;
    let connection = rusqlite::Connection::open(&copied).map_err(|e| Error::auth(e.to_string()))?;
    let mut stmt = connection
        .prepare("SELECT host_key, name, value, encrypted_value FROM cookies")
        .map_err(|e| Error::auth(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, Vec<u8>>(3).unwrap_or_default(),
            ))
        })
        .map_err(|e| Error::auth(e.to_string()))?;
    let mut by_name: HashMap<String, (i32, String)> = HashMap::new();
    for row in rows.flatten() {
        let (host, name, value, encrypted) = row;
        if !host.contains("youtube.com") {
            continue;
        }
        let name = name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let mut decoded = value;
        if !encrypted.is_empty() {
            match decrypt_chrome_value(&encrypted, &key) {
                Ok(plain) => decoded = plain,
                Err(_) if decoded.is_empty() => continue,
                Err(_) => {}
            }
        }
        if decoded.is_empty() {
            continue;
        }
        let preference = host_preference(&host);
        let previous = by_name.get(&name).map(|(p, _)| *p).unwrap_or(-1);
        if preference >= previous {
            by_name.insert(name, (preference, decoded));
        }
    }
    Ok(by_name
        .into_iter()
        .map(|(name, (_pref, value))| (name, value))
        .collect())
}

pub fn os_crypt_password(application: &str) -> Result<Vec<u8>> {
    if which("secret-tool").is_none() {
        return Err(Error::auth(
            "secret-tool is missing. Install libsecret to read Chromium cookies, or paste request headers instead.",
        ));
    }
    let schema = format!("{application}_libsecret_os_crypt_password_v2");
    let lookups = [
        vec!["lookup", "application", application],
        vec!["lookup", "xdg:schema", schema.as_str()],
        vec![
            "lookup",
            "xdg:schema",
            "chrome_libsecret_os_crypt_password_v2",
        ],
        vec![
            "lookup",
            "xdg:schema",
            "chromium_libsecret_os_crypt_password_v2",
        ],
    ];
    for command in lookups {
        let output = Command::new("secret-tool").args(command).output();
        if let Ok(output) = output {
            if output.status.success() && !output.stdout.is_empty() {
                return Ok(output.stdout);
            }
        }
    }
    Err(Error::auth(
        "Could not unlock the Chromium cookie key. Sign in to Chromium once, then try again.",
    ))
}

pub fn derive_os_crypt_key(password: &[u8]) -> Vec<u8> {
    let mut key = vec![0u8; CHROME_KEY_LEN];
    pbkdf2_hmac::<Sha1>(password, CHROME_KEY_SALT, CHROME_PBKDF2_ROUNDS, &mut key);
    key
}

pub fn aes_cbc_decrypt(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    let mut buf = ciphertext.to_vec();
    let dec = Aes128CbcDec::new_from_slices(key, iv).map_err(|e| Error::auth(e.to_string()))?;
    let pt = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| Error::auth(e.to_string()))?;
    Ok(pt.to_vec())
}

pub fn unpad_pkcs7(data: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() {
        return Err(Error::invalid("empty PKCS7 payload"));
    }
    let pad = data[data.len() - 1] as usize;
    if pad < 1 || pad > 16 || data.len() < pad || data[data.len() - pad..] != vec![pad as u8; pad] {
        return Err(Error::invalid("invalid PKCS7 padding"));
    }
    Ok(data[..data.len() - pad].to_vec())
}

pub fn decrypt_chrome_value(encrypted: &[u8], key: &[u8]) -> Result<String> {
    if encrypted.is_empty() {
        return Ok(String::new());
    }
    let payload = if encrypted.starts_with(b"v10") || encrypted.starts_with(b"v11") {
        &encrypted[3..]
    } else {
        encrypted
    };
    if payload.len() < 16 || payload.len() % 16 != 0 {
        return Err(Error::invalid("unsupported Chromium cookie ciphertext"));
    }
    let mut buf = payload.to_vec();
    let dec =
        Aes128CbcDec::new_from_slices(key, CHROME_IV).map_err(|e| Error::auth(e.to_string()))?;
    let raw = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| Error::auth(e.to_string()))?;
    let plain = raw.to_vec();
    if plain.len() > CHROME_HASH_PREFIX_LEN {
        let (digest, rest) = plain.split_at(CHROME_HASH_PREFIX_LEN);
        if Sha256::digest(rest).as_slice() == digest {
            return Ok(String::from_utf8_lossy(rest).into_owned());
        }
        if let Ok(text) = std::str::from_utf8(rest) {
            return Ok(text.to_string());
        }
    }
    Ok(String::from_utf8_lossy(&plain).into_owned())
}

fn copy_sqlite(src: &Path, directory: &Path) -> Result<PathBuf> {
    let dest = directory.join("Cookies");
    fs::copy(src, &dest)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let extra = PathBuf::from(format!("{}{suffix}", src.display()));
        if extra.is_file() {
            let _ = fs::copy(&extra, directory.join(format!("Cookies{suffix}")));
        }
    }
    Ok(dest)
}

fn browser_rank(keyring: &str) -> i32 {
    match keyring {
        "chromium" => 3,
        "chrome" => 2,
        "brave" => 1,
        _ => 0,
    }
}

fn host_preference(host: &str) -> i32 {
    if host.ends_with("music.youtube.com") {
        3
    } else if host == ".youtube.com" || host == "youtube.com" {
        2
    } else if host.contains("youtube.com") {
        1
    } else {
        0
    }
}

pub fn redact_err(err: impl std::fmt::Display) -> String {
    redact(&err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use tempfile::tempdir;

    fn pad_pkcs7(data: &[u8]) -> Vec<u8> {
        let pad = 16 - (data.len() % 16);
        let mut out = data.to_vec();
        out.extend(std::iter::repeat(pad as u8).take(pad));
        out
    }

    fn aes_cbc_encrypt(plaintext: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
        use cipher::generic_array::GenericArray;
        use cipher::{BlockEncrypt, KeyInit};
        type Aes128CbcEnc = cbc::Encryptor<Aes128>;
        let mut buf = pad_pkcs7(plaintext);
        let enc = Aes128CbcEnc::new_from_slices(key, iv).unwrap();
        let len = buf.len();
        let cipher = aes::Aes128::new_from_slice(key).unwrap();
        let mut prev = iv.to_vec();
        for chunk in buf.chunks_mut(16) {
            for (b, p) in chunk.iter_mut().zip(prev.iter()) {
                *b ^= *p;
            }
            let mut block = GenericArray::clone_from_slice(chunk);
            cipher.encrypt_block(&mut block);
            chunk.copy_from_slice(&block);
            prev = chunk.to_vec();
        }
        let _ = (enc, len);
        buf
    }

    fn v11_cookie(value: &str, key: &[u8]) -> Vec<u8> {
        let payload = value.as_bytes();
        let digest = Sha256::digest(payload);
        let mut hashed = digest.to_vec();
        hashed.extend_from_slice(payload);
        let mut out = b"v11".to_vec();
        out.extend(aes_cbc_encrypt(&hashed, key, CHROME_IV));
        out
    }

    #[test]
    fn parse_cookie_header_pairs() {
        let pairs = parse_cookie_header("SID=abc; HSID=xyz;  ; broken");
        assert_eq!(
            pairs,
            vec![("SID".into(), "abc".into()), ("HSID".into(), "xyz".into())]
        );
    }

    #[test]
    fn netscape_export() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("cookies.txt");
        write_netscape_cookies("SID=secret; __Secure-1PSID=tok", &dest).unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        assert!(text.contains("SID\tsecret"));
        assert!(text.contains("__Secure-1PSID\ttok"));
        assert!(text.contains(".youtube.com"));
        let mode = fs::metadata(&dest).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(mode.mode() & 0o777, 0o600);
    }

    #[test]
    fn headers_raw_from_cookies_includes_hash() {
        let raw = headers_raw_from_cookies(
            &[
                ("SID".into(), "abc".into()),
                ("__Secure-3PAPISID".into(), "tok".into()),
            ],
            "0",
        );
        assert!(raw.contains("cookie: SID=abc; __Secure-3PAPISID=tok"));
        assert!(raw.contains("x-goog-authuser: 0"));
        assert!(raw.contains("authorization: SAPISIDHASH "));
        assert!(raw.contains("origin: https://music.youtube.com"));
    }

    #[test]
    fn unpad_pkcs7_ok_and_bad() {
        let mut data = b"hello".to_vec();
        data.extend(std::iter::repeat(11).take(11));
        assert_eq!(unpad_pkcs7(&data).unwrap(), b"hello");
        assert!(unpad_pkcs7(b"hello").is_err());
    }

    #[test]
    fn decrypt_chrome_value_v11_hash_prefix() {
        let password = b"test-os-crypt-password";
        let key = derive_os_crypt_key(password);
        let encrypted = v11_cookie("SAPISID-value", &key);
        assert_eq!(
            decrypt_chrome_value(&encrypted, &key).unwrap(),
            "SAPISID-value"
        );
        assert!(!aes_cbc_decrypt(&encrypted[3..], &key, CHROME_IV)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn extract_youtube_cookies_from_sqlite() {
        let password = b"unit-test-key";
        let key = derive_os_crypt_key(password);
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("Cookies");
        let connection = rusqlite::Connection::open(&db_path).unwrap();
        connection
            .execute(
                "CREATE TABLE cookies (host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB)",
                [],
            )
            .unwrap();
        let rows = [
            (
                ".youtube.com",
                "__Secure-3PAPISID",
                "",
                v11_cookie("papisid", &key),
            ),
            (".youtube.com", "SID", "", v11_cookie("sid-value", &key)),
            (".google.com", "SID", "", v11_cookie("google-sid", &key)),
            (".youtube.com", "PREF", "plain", vec![]),
        ];
        for (host, name, value, enc) in rows {
            connection
                .execute(
                    "INSERT INTO cookies VALUES (?, ?, ?, ?)",
                    rusqlite::params![host, name, value, enc],
                )
                .unwrap();
        }
        let database = CookieDatabase {
            keyring: "chromium".into(),
            browser: "Chromium".into(),
            profile: "Default".into(),
            path: db_path,
        };
        let mut passwords = HashMap::new();
        passwords.insert("chromium".into(), password.to_vec());
        let (pairs, source) =
            extract_youtube_cookies(Some(&[database.clone()]), Some(&passwords)).unwrap();
        assert_eq!(source.path, database.path);
        let cookies: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(cookies.get("__Secure-3PAPISID").unwrap(), "papisid");
        assert_eq!(cookies.get("SID").unwrap(), "sid-value");
        assert_eq!(cookies.get("PREF").unwrap(), "plain");
        assert!(!cookies.values().any(|v| v == "google-sid"));
    }

    #[test]
    fn extract_requires_cookie_database() {
        assert!(extract_youtube_cookies(Some(&[]), None).is_err());
    }

    #[test]
    fn refresh_browser_authorization_rewrites_a_stale_hash() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        let path = paths.auth_path();
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "cookie": "SID=abc; __Secure-3PAPISID=tok",
                "origin": "https://music.youtube.com",
                "x-origin": "https://music.youtube.com",
                "authorization": "SAPISIDHASH 1000000000_deadbeef",
            }))
            .unwrap(),
        )
        .unwrap();
        refresh_browser_authorization(&path).unwrap();
        let data: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(data["authorization"]
            .as_str()
            .unwrap()
            .contains("SAPISIDHASH "));
        assert!(!data["authorization"]
            .as_str()
            .unwrap()
            .contains("1000000000_deadbeef"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
