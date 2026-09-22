//! Blocking filesystem helpers. Call through `spawn_blocking` from async code.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone)]
pub struct Found {
    /// Path relative to the walked root, `/`-separated.
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub size: i64,
    /// Unix seconds.
    pub mtime: i64,
}

pub struct Walk {
    pub files: Vec<Found>,
    /// True if any directory could not be read; the listing is then incomplete
    /// and must not be used to conclude that files are gone.
    pub incomplete: bool,
}

pub fn mtime_secs(md: &fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Recursively list regular files. Symlinks, dotfiles and `.part` files are
/// ignored. A missing or unreadable root is an error, not an empty listing.
pub fn walk(root: &Path) -> io::Result<Walk> {
    let mut files = Vec::new();
    let mut incomplete = false;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if dir == root => return Err(e),
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "cannot read directory");
                incomplete = true;
                continue;
            }
        };
        for entry in rd {
            let Ok(entry) = entry else {
                incomplete = true;
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name.ends_with(".part") {
                continue;
            }
            let Ok(ft) = entry.file_type() else {
                incomplete = true;
                continue;
            };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                let Ok(md) = entry.metadata() else {
                    incomplete = true;
                    continue;
                };
                let Ok(rel) = path.strip_prefix(root) else {
                    continue;
                };
                let rel_path = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                files.push(Found {
                    rel_path,
                    abs_path: path,
                    size: md.len() as i64,
                    mtime: mtime_secs(&md),
                });
            }
        }
    }
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(Walk { files, incomplete })
}

/// Streaming BLAKE3 of a file, hex-encoded.
pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Read a whole file, refusing anything larger than `max` before allocating.
pub fn read_capped(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let len = fs::metadata(path)?.len();
    if len > max {
        return Err(io::Error::other(format!(
            "file is {len} bytes, limit is {max}"
        )));
    }
    fs::read(path)
}

/// Write via a temp file in the same directory, then rename, so readers never
/// see a partial file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

/// Rename, falling back to copy + remove when source and destination are on
/// different filesystems.
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(from, to)?;
            fs::remove_file(from)
        }
    }
}

/// `dir/stem.ext`, or `dir/stem-1.ext`, `-2`, ... if that exists.
pub fn free_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let make = |n: u32| {
        let name = if n == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        dir.join(name)
    };
    (0..)
        .map(make)
        .find(|p| !p.exists())
        .expect("unbounded range")
}

/// Remove every derivative file for a photograph.
pub fn remove_derivatives(layout: &photoframe_store::Layout, hash: &str) {
    let Some(dir) = layout.derivative_dir(hash) else {
        return;
    };
    let prefix = format!("{hash}-");
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// After a file leaves `root/<dir>/`, drop the directory if that emptied it.
/// Best effort: a non-empty directory (or a race) simply stays.
pub fn prune_empty_parent(file: &Path, root: &Path) {
    if let Some(parent) = file.parent()
        && parent != root
        && parent.starts_with(root)
    {
        let _ = fs::remove_dir(parent);
    }
}

/// Remove derivative files for `hash` that are not part of the set for
/// `rotation` (left over from before a curated rotation changed).
pub fn prune_other_rotations(layout: &photoframe_store::Layout, hash: &str, rotation: i32) {
    let Some(dir) = layout.derivative_dir(hash) else {
        return;
    };
    let keep: Vec<std::ffi::OsString> = imagepipe::VARIANTS
        .iter()
        .filter_map(|v| layout.derivative_path(hash, v.name, rotation, "jpg"))
        .filter_map(|p| p.file_name().map(ToOwned::to_owned))
        .collect();
    let prefix = format!("{hash}-");
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with(&prefix) && !keep.contains(&name) {
            let _ = fs::remove_file(e.path());
        }
    }
}
