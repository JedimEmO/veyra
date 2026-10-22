//! The data bundle: every file the game reads through [`crate::files`], packed
//! into one blob a browser can fetch in a single request (ticket 0013).
//!
//! The format is the smallest that holds a list of named byte strings, all
//! little endian: the magic `VBUN`, a `u32` version, a `u32` count, then per
//! file a `u32` path length, the UTF-8 path, a `u64` byte length and the bytes.
//! Compression is not part of it: the packer gzips the whole blob and the
//! page unzips it before the app starts, which keeps this module free of a
//! codec and of any dependency.

use slotted_registry::loader::InMemorySource;

/// The first four bytes of a bundle.
pub const MAGIC: [u8; 4] = *b"VBUN";

/// The layout version; a reader refuses any other.
pub const VERSION: u32 = 1;

/// Why a bundle could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BundleError {
    /// The blob does not start with [`MAGIC`] (a gzipped bundle that was not unzipped looks like this).
    #[error("not a data bundle (bad magic)")]
    Magic,
    /// The blob is of a layout version this reader does not know.
    #[error("data bundle version {0}, this build reads {VERSION}")]
    Version(u32),
    /// The blob ends inside an entry.
    #[error("data bundle is truncated")]
    Truncated,
    /// A path is not UTF-8.
    #[error("data bundle holds a path that is not UTF-8")]
    Path,
}

/// Packs `entries` (logical path, bytes) into a bundle.
///
/// # Panics
/// If there are more than `u32::MAX` files or a path longer than that.
#[must_use]
pub fn write(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let bytes: usize = entries.iter().map(|(p, b)| p.len() + b.len() + 12).sum();
    let mut out = Vec::with_capacity(bytes + 12);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(entries.len())
            .expect("file count fits u32")
            .to_le_bytes(),
    );
    for (path, data) in entries {
        out.extend_from_slice(
            &u32::try_from(path.len())
                .expect("path fits u32")
                .to_le_bytes(),
        );
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(data);
    }
    out
}

/// Unpacks a bundle into its (logical path, bytes) entries, in file order.
///
/// # Errors
/// [`BundleError`] when the blob is not a whole bundle of this version.
pub fn read(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, BundleError> {
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.take(4)? != MAGIC {
        return Err(BundleError::Magic);
    }
    let version = cursor.u32()?;
    if version != VERSION {
        return Err(BundleError::Version(version));
    }
    let count = cursor.u32()? as usize;
    // Not `with_capacity(count)`: a corrupt count must not allocate.
    let mut entries = Vec::new();
    for _ in 0..count {
        let path_len = cursor.u32()? as usize;
        let path = std::str::from_utf8(cursor.take(path_len)?)
            .map_err(|_| BundleError::Path)?
            .to_owned();
        let len = usize::try_from(cursor.u64()?).map_err(|_| BundleError::Truncated)?;
        entries.push((path, cursor.take(len)?.to_vec()));
    }
    Ok(entries)
}

/// [`read`] for a blob already in memory and not wanted again: each entry is
/// split off the end and the blob shrunk behind it, so the peak is the blob
/// and one entry rather than two copies of everything (a browser's memory
/// never shrinks, and the data is 185 MB).
///
/// # Errors
/// As [`read`].
pub fn read_owned(mut bytes: Vec<u8>) -> Result<Vec<(String, Vec<u8>)>, BundleError> {
    let mut spans = Vec::new();
    let mut paths = Vec::new();
    {
        let mut cursor = Cursor {
            bytes: &bytes,
            at: 0,
        };
        if cursor.take(4)? != MAGIC {
            return Err(BundleError::Magic);
        }
        let version = cursor.u32()?;
        if version != VERSION {
            return Err(BundleError::Version(version));
        }
        for _ in 0..cursor.u32()? {
            let path_len = cursor.u32()? as usize;
            paths.push(
                std::str::from_utf8(cursor.take(path_len)?)
                    .map_err(|_| BundleError::Path)?
                    .to_owned(),
            );
            let len = usize::try_from(cursor.u64()?).map_err(|_| BundleError::Truncated)?;
            let start = cursor.at;
            cursor.take(len)?;
            spans.push((start, len));
        }
    }
    let mut entries = Vec::with_capacity(paths.len());
    for (path, (start, len)) in paths.into_iter().zip(spans).rev() {
        // What follows the entry is the next entry's header, which goes.
        let mut data = bytes.split_off(start);
        data.truncate(len);
        data.shrink_to_fit();
        entries.push((path, data));
        bytes.shrink_to_fit();
    }
    entries.reverse();
    Ok(entries)
}

/// A bundle as the [`AssetSource`](slotted_registry::loader::AssetSource) the
/// files port installs, on the web and for `--data-bundle`.
///
/// # Errors
/// As [`read`].
pub fn source(bytes: &[u8]) -> Result<InMemorySource, BundleError> {
    Ok(read(bytes)?.into_iter().collect())
}

/// [`source`] for bytes not wanted again, as [`read_owned`].
///
/// # Errors
/// As [`read`].
pub fn source_owned(bytes: Vec<u8>) -> Result<InMemorySource, BundleError> {
    Ok(read_owned(bytes)?.into_iter().collect())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BundleError> {
        let end = self.at.checked_add(n).ok_or(BundleError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(BundleError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, BundleError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, BundleError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotted_registry::loader::AssetSource;

    fn entries() -> Vec<(String, Vec<u8>)> {
        vec![
            ("assets/a/one.ron".into(), b"(1)".to_vec()),
            ("generated/b/two.ashlar".into(), vec![0, 255, 7]),
            ("assets/empty.json".into(), Vec::new()),
        ]
    }

    #[test]
    fn a_bundle_reads_back_what_was_written() {
        assert_eq!(read(&write(&entries())), Ok(entries()));
    }

    #[test]
    fn an_owned_bundle_reads_the_same() {
        assert_eq!(read_owned(write(&entries())), Ok(entries()));
        let good = write(&entries());
        assert_eq!(
            read_owned(good[..good.len() - 1].to_vec()),
            Err(BundleError::Truncated)
        );
    }

    #[test]
    fn the_source_lists_and_reads() {
        let source = source(&write(&entries())).expect("reads");
        assert_eq!(
            source.list("assets/a").expect("lists"),
            ["assets/a/one.ron"]
        );
        assert_eq!(
            source.read("generated/b/two.ashlar").expect("reads"),
            [0, 255, 7]
        );
    }

    #[test]
    fn a_damaged_bundle_is_refused() {
        let good = write(&entries());
        assert_eq!(read(&good[..good.len() - 1]), Err(BundleError::Truncated));
        assert_eq!(read(b"\x1f\x8b\x08\0\0\0\0\0"), Err(BundleError::Magic));
        let mut other = good.clone();
        other[4] = 9;
        assert_eq!(read(&other), Err(BundleError::Version(9)));
    }
}
