use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Application id used for XDG dirs, sockets, and the mpv client name.
pub const APP_ID: &str = "omamusic";
pub const LEGACY_APP_ID: &str = "omarchy-ytmusic";
pub const YTMUSICBAR_APP_ID: &str = "ytmusicbar";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl AppPaths {
    pub fn from_env() -> Self {
        Self {
            config_dir: xdg_home("XDG_CONFIG_HOME", ".config").join(APP_ID),
            cache_dir: xdg_home("XDG_CACHE_HOME", ".cache").join(APP_ID),
            runtime_dir: runtime_root().join(APP_ID),
        }
    }

    pub fn for_tests(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            config_dir: root.join("config"),
            cache_dir: root.join("cache"),
            runtime_dir: root.join("runtime"),
        }
    }

    pub fn ensure(&self) -> io::Result<()> {
        for dir in [&self.config_dir, &self.cache_dir, &self.runtime_dir] {
            fs::create_dir_all(dir)?;
            chmod(dir, 0o700);
        }
        Ok(())
    }

    pub fn auth_path(&self) -> PathBuf {
        self.config_dir.join("browser.json")
    }

    pub fn oauth_path(&self) -> PathBuf {
        self.config_dir.join("oauth.json")
    }

    pub fn oauth_client_path(&self) -> PathBuf {
        self.config_dir.join("oauth-client.json")
    }

    pub fn history_path(&self) -> PathBuf {
        self.config_dir.join("play-history.json")
    }

    pub fn queue_path(&self) -> PathBuf {
        self.config_dir.join("play-queue.json")
    }

    /// Load history from this app dir, or a leftover Python plugin session.
    pub fn history_load_path(&self) -> PathBuf {
        first_existing_file(
            &self.history_path(),
            &self.legacy_config_files("play-history.json"),
        )
    }

    /// Load the queue from this app dir, or a leftover Python plugin session.
    pub fn queue_load_path(&self) -> PathBuf {
        first_existing_file(
            &self.queue_path(),
            &self.legacy_config_files("play-queue.json"),
        )
    }

    pub fn socket_path(&self) -> PathBuf {
        self.runtime_dir.join("backend.sock")
    }

    pub fn mpv_socket(&self) -> PathBuf {
        self.runtime_dir.join("mpv.sock")
    }

    pub fn cookies_path(&self) -> PathBuf {
        self.cache_dir.join("cookies.txt")
    }

    pub fn legacy_auth_candidates(&self) -> Vec<PathBuf> {
        self.legacy_config_files("browser.json")
    }

    fn legacy_config_files(&self, name: &str) -> Vec<PathBuf> {
        let config_home = self
            .config_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| xdg_home("XDG_CONFIG_HOME", ".config"));
        vec![
            config_home.join(LEGACY_APP_ID).join(name),
            config_home.join(YTMUSICBAR_APP_ID).join(name),
        ]
    }
}

pub fn first_existing_file(preferred: &Path, candidates: &[PathBuf]) -> PathBuf {
    if file_nonempty(preferred) {
        return preferred.to_path_buf();
    }
    for candidate in candidates {
        if file_nonempty(candidate) {
            return candidate.clone();
        }
    }
    preferred.to_path_buf()
}

fn file_nonempty(path: &Path) -> bool {
    path.is_file() && path.metadata().map(|m| m.len() > 2).unwrap_or(false)
}

pub fn xdg_home(var: &str, fallback: &str) -> PathBuf {
    env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(fallback))
}

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn runtime_root() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let uid = uid();
            env::temp_dir().join(format!("{APP_ID}-{uid}"))
        })
}

fn uid() -> u32 {
    libc_uid()
}

fn libc_uid() -> u32 {
    // Avoid a libc dependency: read /proc/self/status.
    if let Ok(text) = fs::read_to_string("/proc/self/status") {
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(first) = rest.split_whitespace().next() {
                    if let Ok(id) = first.parse() {
                        return id;
                    }
                }
            }
        }
    }
    0
}

pub fn chmod(path: &Path, mode: u32) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(mode);
        let _ = fs::set_permissions(path, perms);
    }
}

pub fn yt_dlp_sigfunc_cache() -> PathBuf {
    xdg_home("XDG_CACHE_HOME", ".cache")
        .join("yt-dlp")
        .join("youtube-sigfuncs")
}

pub fn which(bin: &str) -> Option<PathBuf> {
    if let Ok(path) = env::var("PATH") {
        for dir in path.split(':') {
            let candidate = Path::new(dir).join(bin);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn first_existing_file_prefers_native_then_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("play-queue.json");
        let legacy = dir.path().join("legacy.json");
        fs::write(&legacy, "{\"items\":[]}\n").unwrap();
        assert_eq!(
            first_existing_file(&native, &[legacy.clone()]),
            legacy
        );
        fs::write(&native, "{\"items\":[1]}\n").unwrap();
        assert_eq!(
            first_existing_file(&native, &[legacy]),
            native
        );
    }

    #[test]
    fn first_existing_file_keeps_preferred_when_nothing_exists() {
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("play-queue.json");
        let missing = dir.path().join("missing.json");
        assert_eq!(first_existing_file(&native, &[missing]), native);
    }
}
