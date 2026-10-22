//! Where the game reads its data from at run time: one installed
//! [`AssetSource`] with logical paths `assets/...` and `generated/...`
//! (ticket 0013).
//!
//! Natively the adapter is [`MountedDirs`], two directories under those two
//! names; on the web it is an in-memory source filled from a fetched bundle.
//! A loader below the render layer takes a [`Files`] (or any `&dyn
//! AssetSource`) and never learns which, and a test hands it a source of its
//! own instead of installing one, so tests running together never share one.
//! Large binary assets (textures, meshes, sounds) are not read here: they go
//! through Bevy's asset server.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use slotted_registry::loader::{AssetSource, DirSource, SourceError};

/// A source that can be shared between threads, the type [`install`] takes.
pub type Source = dyn AssetSource + Send + Sync;

static INSTALLED: RwLock<Option<&'static Source>> = RwLock::new(None);

/// Installs the process-wide source, replacing an earlier one. Called once at
/// startup, before anything reads. The source lives for the rest of the
/// process, so a later install leaks the first, which is a few bytes.
pub fn install(source: Box<Source>) {
    let leaked: &'static Source = Box::leak(source);
    *INSTALLED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(leaked);
}

/// Whether [`install`] has been called.
#[must_use]
pub fn is_installed() -> bool {
    INSTALLED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
}

/// The installed source.
///
/// # Panics
///
/// If nothing was installed: a read of game data before startup finished is
/// a bug, and an empty source would only show up as an empty world.
#[must_use]
pub fn current() -> &'static Source {
    INSTALLED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .expect("no AssetSource installed: call strata_registry::files::install at startup")
}

/// A cheap, cloneable handle on a source, so a resource or a task can own one.
#[derive(Clone)]
pub struct Files(Arc<Source>);

impl std::fmt::Debug for Files {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Files")
    }
}

impl Files {
    /// A handle on `source`.
    pub fn new(source: impl AssetSource + Send + Sync + 'static) -> Self {
        Self(Arc::new(source))
    }

    /// A handle on the directory `root` (paths are relative to it), the
    /// adapter a test over a temporary directory uses.
    pub fn dir(root: impl Into<PathBuf>) -> Self {
        Self::new(DirSource::new(root))
    }

    /// A handle on the installed source, whole.
    ///
    /// # Panics
    ///
    /// If nothing was installed.
    #[must_use]
    pub fn installed() -> Self {
        Self::new(Installed(current()))
    }

    /// This source seen from `prefix` down: `files.under("assets")` reads
    /// `assets/biomes/x.ron` as `biomes/x.ron`.
    #[must_use]
    pub fn under(&self, prefix: &str) -> Self {
        Self::new(Under {
            inner: self.clone(),
            prefix: prefix.trim_matches('/').to_owned(),
        })
    }

    /// One file as text.
    ///
    /// # Errors
    ///
    /// [`SourceError::NotFound`] if there is no such file; an `Io` error if it
    /// is not UTF-8.
    pub fn read_string(&self, path: &str) -> Result<String, SourceError> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes).map_err(|error| SourceError::Io {
            path: path.to_owned(),
            message: error.to_string(),
        })
    }

    /// Whether `path` is there.
    ///
    /// Answered from the parent directory's listing, so a baked building is
    /// not read whole (and, on the web, copied) just to learn it exists.
    #[must_use]
    pub fn exists(&self, path: &str) -> bool {
        match path.rsplit_once('/') {
            Some((parent, _)) => self
                .list(parent)
                .is_ok_and(|listed| listed.iter().any(|entry| entry == path)),
            None => self.read(path).is_ok(),
        }
    }
}

impl AssetSource for Files {
    fn list(&self, dir: &str) -> Result<Vec<String>, SourceError> {
        self.0.list(dir)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, SourceError> {
        self.0.read(path)
    }
}

/// The installed source as a value.
struct Installed(&'static Source);

impl AssetSource for Installed {
    fn list(&self, dir: &str) -> Result<Vec<String>, SourceError> {
        self.0.list(dir)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, SourceError> {
        self.0.read(path)
    }
}

/// A source seen from a prefix down.
struct Under {
    inner: Files,
    prefix: String,
}

impl AssetSource for Under {
    fn list(&self, dir: &str) -> Result<Vec<String>, SourceError> {
        let full = join(&self.prefix, dir);
        let strip = format!("{}/", self.prefix);
        Ok(self
            .inner
            .list(&full)?
            .into_iter()
            .map(|path| {
                path.strip_prefix(&strip)
                    .map_or(path.clone(), str::to_owned)
            })
            .collect())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, SourceError> {
        self.inner.read(&join(&self.prefix, path))
    }
}

fn join(prefix: &str, path: &str) -> String {
    let path = path.trim_matches('/');
    if path.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}/{path}")
    }
}

/// The native adapter: directories mounted under logical names, so the game's
/// `--assets` directory is `assets/...` and its `generated/` is
/// `generated/...`.
#[derive(Debug, Clone, Default)]
pub struct MountedDirs {
    mounts: Vec<(String, DirSource)>,
}

impl MountedDirs {
    /// No mounts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mounts `dir` as `name`.
    #[must_use]
    pub fn mount(mut self, name: &str, dir: impl Into<PathBuf>) -> Self {
        self.mounts
            .push((name.trim_matches('/').to_owned(), DirSource::new(dir)));
        self
    }

    /// The mount `path` falls under and the path inside it.
    fn split<'a>(&self, path: &'a str) -> Option<(&str, &DirSource, &'a str)> {
        let path = path.trim_start_matches('/');
        self.mounts.iter().find_map(|(name, dir)| {
            let rest = path.strip_prefix(name.as_str())?;
            (rest.is_empty() || rest.starts_with('/')).then_some((name.as_str(), dir, rest))
        })
    }
}

impl AssetSource for MountedDirs {
    fn list(&self, dir: &str) -> Result<Vec<String>, SourceError> {
        let Some((name, mount, rest)) = self.split(dir) else {
            return Ok(Vec::new());
        };
        Ok(mount
            .list(rest)?
            .into_iter()
            .map(|path| format!("{name}/{}", path.trim_start_matches('/')))
            .collect())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, SourceError> {
        match self.split(path) {
            Some((_, mount, rest)) => mount.read(rest).map_err(|error| match error {
                SourceError::NotFound(_) => SourceError::NotFound(path.to_owned()),
                other @ SourceError::Io { .. } => other,
            }),
            None => Err(SourceError::NotFound(path.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotted_registry::loader::InMemorySource;

    fn files() -> Files {
        Files::new(
            InMemorySource::new()
                .with("assets/a/one.ron", "1")
                .with("assets/a/two.ron", "2")
                .with("generated/b.ron", "3"),
        )
    }

    #[test]
    fn under_reads_and_lists_from_its_prefix() {
        let assets = files().under("assets");
        assert_eq!(assets.read_string("a/one.ron").expect("ok"), "1");
        assert_eq!(assets.list("a").expect("ok"), ["a/one.ron", "a/two.ron"]);
        assert!(!assets.exists("b.ron"));
        assert!(files().under("generated").exists("b.ron"));
        assert!(assets.exists("a/one.ron") && !assets.exists("a/three.ron"));
    }

    #[test]
    fn mounted_dirs_serve_their_names_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("strata-mounts-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("x")).expect("ok");
        std::fs::write(dir.join("x/f.txt"), "hi").expect("ok");
        let mounted = Files::new(MountedDirs::new().mount("assets", &dir));
        assert_eq!(mounted.read_string("assets/x/f.txt").expect("ok"), "hi");
        assert_eq!(mounted.under("assets").list("x").expect("ok"), ["x/f.txt"]);
        assert_eq!(mounted.list("assets/x").expect("ok"), ["assets/x/f.txt"]);
        assert_eq!(
            mounted.read("assetsx/f").expect_err("not mounted"),
            SourceError::NotFound("assetsx/f".to_owned())
        );
        assert!(mounted.read("generated/x").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
