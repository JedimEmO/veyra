//! Where the game keeps what the player writes: the household file and the
//! settings (ticket 0013). One installed [`Store`], keyed by a string.
//!
//! Natively the adapter is [`FileStore`] and a key is a path, so the files are
//! exactly where they were before the port existed; the web build installs a
//! store over the browser's local storage. A developer's output (a still's
//! report, a render look export) is not saved through here and stays native.

use std::sync::RwLock;

/// A place to keep named text.
pub trait Store: Send + Sync {
    /// The text under `key`, or `None` when nothing was ever written there.
    ///
    /// # Errors
    /// The store could not be read for another reason than being empty.
    fn read(&self, key: &str) -> Result<Option<String>, String>;

    /// Writes `text` under `key`, replacing what was there.
    ///
    /// # Errors
    /// The text could not be kept.
    fn write(&self, key: &str, text: &str) -> Result<(), String>;
}

/// Files on disk, a key being the path. Writing makes the parent directory
/// and replaces the file atomically.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, Clone, Copy)]
pub struct FileStore;

#[cfg(not(target_arch = "wasm32"))]
impl Store for FileStore {
    fn read(&self, key: &str) -> Result<Option<String>, String> {
        match std::fs::read_to_string(key) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn write(&self, key: &str, text: &str) -> Result<(), String> {
        let path = std::path::Path::new(key);
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        // Written beside the target, synced, then renamed over it: a crash
        // leaves the old file or the new one, never an empty or half one.
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        let temp = std::path::PathBuf::from(temp);
        let kept = (|| {
            use std::io::Write;
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        kept.map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            format!("{key}: {e}")
        })
    }
}

/// The browser's `localStorage`, a key being the key. It holds a few
/// megabytes per origin, which a household file is far under.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalStorage;

#[cfg(target_arch = "wasm32")]
impl LocalStorage {
    fn storage() -> Result<web_sys::Storage, String> {
        web_sys::window()
            .ok_or_else(|| "no window".to_owned())?
            .local_storage()
            .map_err(|e| format!("localStorage: {e:?}"))?
            .ok_or_else(|| "localStorage is not available".to_owned())
    }
}

#[cfg(target_arch = "wasm32")]
impl Store for LocalStorage {
    fn read(&self, key: &str) -> Result<Option<String>, String> {
        Self::storage()?
            .get_item(key)
            .map_err(|e| format!("{key}: {e:?}"))
    }

    fn write(&self, key: &str, text: &str) -> Result<(), String> {
        // A full quota is an error the caller words, not a panic.
        Self::storage()?
            .set_item(key, text)
            .map_err(|e| format!("{key}: {e:?}"))
    }
}

/// Keeps nothing and says so: the store of a build that installed none on a
/// target with no files.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullStore;

impl Store for NullStore {
    fn read(&self, _key: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn write(&self, key: &str, _text: &str) -> Result<(), String> {
        Err(format!("{key}: no store installed"))
    }
}

static INSTALLED: RwLock<Option<&'static dyn Store>> = RwLock::new(None);

/// Installs the process-wide store, replacing an earlier one (which is
/// leaked: a few bytes, once).
pub fn install(store: Box<dyn Store>) {
    let leaked: &'static dyn Store = Box::leak(store);
    *INSTALLED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(leaked);
}

/// The installed store; with none installed, the files natively and
/// [`NullStore`] elsewhere, so a test and a tool need no setup.
#[must_use]
pub fn current() -> &'static dyn Store {
    if let Some(store) = *INSTALLED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return store;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        &FileStore
    }
    #[cfg(target_arch = "wasm32")]
    {
        &NullStore
    }
}
