//! The file system layer that ufoLib works through: a port of the parts of
//! `fontTools.misc.filesystem` (a subset of PyFilesystem2) that ufoLib uses.
//!
//! A [`Fs`] is a directory of either the local disk (`OSFS`) or an in-memory
//! tree (the contents of a zipped UFO, `ReadZipFS`, or a zipped UFO being
//! written, `WriteZipFS` and `TempFS`), seen through an optional
//! subdirectory prefix (`SubFS`).

use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// The errors of the file system layer (`fontTools.misc.filesystem.errors`).
#[derive(Debug)]
pub(crate) enum FsError {
	/// `ResourceNotFound`.
	NotFound(String),
	/// `DirectoryExpected`.
	DirectoryExpected(String),
	/// `FileExpected`.
	FileExpected(String),
	/// `ResourceReadOnly`.
	ReadOnly(String),
	/// `IllegalBackReference`: a path leaving the file system.
	IllegalBackReference(String),
	/// `DirectoryNotEmpty`.
	DirectoryNotEmpty(String),
	/// `DestinationExists` and `FileExistsError`.
	Exists(String),
	/// Other operating system errors (`ResourceError`, `OSError`).
	Io(std::io::Error),
}

impl fmt::Display for FsError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			FsError::NotFound(m)
			| FsError::DirectoryExpected(m)
			| FsError::FileExpected(m)
			| FsError::ReadOnly(m)
			| FsError::IllegalBackReference(m)
			| FsError::DirectoryNotEmpty(m)
			| FsError::Exists(m) => f.write_str(m),
			FsError::Io(e) => write!(f, "{e}"),
		}
	}
}

impl From<FsError> for crate::UfoLibError {
	fn from(e: FsError) -> Self {
		match e {
			FsError::Io(e) => crate::UfoLibError::Io(e),
			other => crate::UfoLibError::Filesystem(other.to_string()),
		}
	}
}

pub(crate) type FsResult<T> = std::result::Result<T, FsError>;

/// An entry of an in-memory tree.
#[derive(Debug, Clone)]
pub(crate) enum MemEntry {
	/// A directory.
	Dir,
	/// A file, with its modification time (seconds since the epoch).
	File(Vec<u8>, Option<f64>),
}

/// An in-memory directory tree, keyed by `/`-separated paths without a
/// leading slash.
#[derive(Debug, Default)]
pub(crate) struct MemFs {
	pub(crate) entries: BTreeMap<String, MemEntry>,
	/// Writing raises `ResourceReadOnly` (`ReadZipFS`).
	pub(crate) read_only: bool,
	/// `str(fs)`.
	pub(crate) label: String,
}

#[derive(Debug, Clone)]
enum Backend {
	Os(Arc<PathBuf>),
	Mem(Arc<Mutex<MemFs>>),
}

/// A directory to read UFO data from and write it to (fontTools' `FS`
/// objects).
#[derive(Debug, Clone)]
pub struct Fs {
	backend: Backend,
	/// The path components of the subdirectory (`SubFS`).
	prefix: Vec<String>,
	/// The root of a directory of the local disk with symbolic links expanded
	/// (`OSFS._root`), which every path must stay in.
	resolved_root: Option<Arc<PathBuf>>,
}

fn lock(m: &Mutex<MemFs>) -> MutexGuard<'_, MemFs> {
	m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now() -> Option<f64> {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.ok()
		.map(|d| d.as_secs_f64())
}

impl Fs {
	/// `OSFS(root, create)`: a directory of the local disk.
	pub(crate) fn os(root: &Path, create: bool) -> FsResult<Fs> {
		if create {
			std::fs::create_dir_all(root).map_err(FsError::Io)?;
		} else if !root.is_dir() {
			return Err(FsError::NotFound(format!(
				"unable to create OSFS: {:?} does not exist or is not a directory",
				root.display().to_string()
			)));
		}
		let root = normalize_os_path(&std::path::absolute(root).map_err(FsError::Io)?);
		// `Path(root).resolve()`.
		let resolved = resolve_under(Path::new(""), root.components().map(|c| c.as_os_str()));
		Ok(Fs {
			backend: Backend::Os(Arc::new(root)),
			prefix: Vec::new(),
			resolved_root: Some(Arc::new(resolved)),
		})
	}

	/// An in-memory tree.
	pub(crate) fn mem(mem: MemFs) -> Fs {
		Fs {
			backend: Backend::Mem(Arc::new(Mutex::new(mem))),
			prefix: Vec::new(),
			resolved_root: None,
		}
	}

	/// The in-memory tree, if this is one.
	pub(crate) fn mem_tree(&self) -> Option<Arc<Mutex<MemFs>>> {
		match &self.backend {
			Backend::Mem(m) => Some(m.clone()),
			Backend::Os(_) => None,
		}
	}

	/// The path components of the subdirectory.
	pub(crate) fn prefix(&self) -> &[String] {
		&self.prefix
	}

	/// The components of `path`, relative to the root of the backend.
	fn components(&self, path: &str) -> FsResult<Vec<String>> {
		let mut parts: Vec<String> = Vec::with_capacity(self.prefix.len() + 2);
		let separators: &[char] = match self.backend {
			Backend::Os(_) => &['/', '\\'],
			Backend::Mem(_) => &['/'],
		};
		if let Backend::Os(_) = self.backend {
			// An absolute path or a drive replaces the root in `Path.__truediv__`.
			let p = Path::new(path.trim_start_matches('/'));
			if p.components().any(|c| matches!(c, Component::Prefix(_))) {
				return Err(FsError::IllegalBackReference(path.to_owned()));
			}
		}
		// Like `SubFS`, a subdirectory forwards the joined path to its parent,
		// which only rejects paths leaving its own root.
		parts.extend(self.prefix.iter().cloned());
		for part in path.split(separators) {
			match part {
				"" | "." => {}
				".." => {
					if parts.pop().is_none() {
						return Err(FsError::IllegalBackReference(path.to_owned()));
					}
				}
				p => parts.push(p.to_owned()),
			}
		}
		// `OSFS._abs`: the path with symbolic links expanded, and `..` applied
		// after them, must stay in the root, so that a link inside the root that
		// points outside it is rejected too. That resolved path is the one used.
		if let Some(root) = &self.resolved_root {
			let rel = self
				.prefix
				.iter()
				.map(OsStr::new)
				.chain(path.split(separators).map(OsStr::new));
			let resolved = resolve_under(root, rel);
			if !is_relative_to(&resolved, root) {
				return Err(FsError::IllegalBackReference(path.to_owned()));
			}
			return Ok(resolved
				.components()
				.skip(root.components().count())
				.map(|c| c.as_os_str().to_string_lossy().into_owned())
				.collect());
		}
		Ok(parts)
	}

	fn os_path(root: &Path, components: &[String]) -> PathBuf {
		let mut p = root.to_path_buf();
		for c in components {
			p.push(c);
		}
		p
	}

	fn key(components: &[String]) -> String {
		components.join("/")
	}

	/// `getsyspath(path)`: `None` for in-memory trees (`NoSysPath`).
	pub(crate) fn getsyspath(&self, path: &str) -> FsResult<Option<PathBuf>> {
		let c = self.components(path)?;
		Ok(match &self.backend {
			Backend::Os(root) => Some(Self::os_path(root, &c)),
			Backend::Mem(_) => None,
		})
	}

	/// `exists(path)`.
	pub(crate) fn exists(&self, path: &str) -> FsResult<bool> {
		let c = self.components(path)?;
		Ok(match &self.backend {
			Backend::Os(root) => Self::os_path(root, &c).exists(),
			Backend::Mem(m) => c.is_empty() || lock(m).entries.contains_key(&Self::key(&c)),
		})
	}

	/// `isdir(path)`.
	pub(crate) fn isdir(&self, path: &str) -> FsResult<bool> {
		let c = self.components(path)?;
		Ok(match &self.backend {
			Backend::Os(root) => Self::os_path(root, &c).is_dir(),
			Backend::Mem(m) => {
				c.is_empty() || matches!(lock(m).entries.get(&Self::key(&c)), Some(MemEntry::Dir))
			}
		})
	}

	/// `isfile(path)`.
	pub(crate) fn isfile(&self, path: &str) -> FsResult<bool> {
		let c = self.components(path)?;
		Ok(match &self.backend {
			Backend::Os(root) => Self::os_path(root, &c).is_file(),
			Backend::Mem(m) => matches!(
				lock(m).entries.get(&Self::key(&c)),
				Some(MemEntry::File(..))
			),
		})
	}

	/// `readbytes(path)`.
	pub(crate) fn readbytes(&self, path: &str) -> FsResult<Vec<u8>> {
		let c = self.components(path)?;
		match &self.backend {
			Backend::Os(root) => {
				let p = Self::os_path(root, &c);
				std::fs::read(&p).map_err(|e| match e.kind() {
					std::io::ErrorKind::NotFound => {
						FsError::NotFound(format!("No such file or directory: {}", py_repr(path)))
					}
					_ if p.is_dir() => {
						FsError::FileExpected(format!("{} is a directory", py_repr(path)))
					}
					_ => FsError::Io(e),
				})
			}
			Backend::Mem(m) => match lock(m).entries.get(&Self::key(&c)) {
				Some(MemEntry::File(data, _)) => Ok(data.clone()),
				Some(MemEntry::Dir) => Err(FsError::FileExpected(format!(
					"{} is a directory",
					py_repr(path)
				))),
				None => Err(FsError::NotFound(format!(
					"No such file or directory: {}",
					py_repr(path)
				))),
			},
		}
	}

	/// `writebytes(path, data)`: the parent directory must exist.
	pub(crate) fn writebytes(&self, path: &str, data: &[u8]) -> FsResult<()> {
		let c = self.components(path)?;
		match &self.backend {
			Backend::Os(root) => {
				let p = Self::os_path(root, &c);
				if p.is_dir() {
					return Err(FsError::FileExpected(format!(
						"{} is a directory",
						py_repr(path)
					)));
				}
				std::fs::write(&p, data).map_err(|e| match e.kind() {
					std::io::ErrorKind::NotFound => {
						FsError::NotFound(format!("No such file or directory: {}", py_repr(path)))
					}
					_ => FsError::Io(e),
				})
			}
			Backend::Mem(m) => {
				let mut m = lock(m);
				if m.read_only {
					return Err(FsError::ReadOnly(path.to_owned()));
				}
				let key = Self::key(&c);
				if matches!(m.entries.get(&key), Some(MemEntry::Dir)) || c.is_empty() {
					return Err(FsError::FileExpected(format!(
						"{} is a directory",
						py_repr(path)
					)));
				}
				let parent = Self::key(&c[..c.len() - 1]);
				if !parent.is_empty() && !matches!(m.entries.get(&parent), Some(MemEntry::Dir)) {
					return Err(FsError::NotFound(format!(
						"No such file or directory: {}",
						py_repr(path)
					)));
				}
				m.entries.insert(key, MemEntry::File(data.to_vec(), now()));
				Ok(())
			}
		}
	}

	/// `remove(path)`: removes a file.
	pub(crate) fn remove(&self, path: &str) -> FsResult<()> {
		let c = self.components(path)?;
		match &self.backend {
			Backend::Os(root) => {
				let p = Self::os_path(root, &c);
				if p.is_dir() {
					return Err(FsError::FileExpected(format!(
						"path {} should be a file",
						py_repr(&p.display().to_string())
					)));
				}
				std::fs::remove_file(&p).map_err(|e| match e.kind() {
					std::io::ErrorKind::NotFound => FsError::NotFound(format!(
						"No such file or directory: {}",
						py_repr(&p.display().to_string())
					)),
					_ => FsError::Io(e),
				})
			}
			Backend::Mem(m) => {
				let mut m = lock(m);
				if m.read_only {
					return Err(FsError::ReadOnly(path.to_owned()));
				}
				let key = Self::key(&c);
				match m.entries.get(&key) {
					Some(MemEntry::File(..)) => {
						m.entries.remove(&key);
						Ok(())
					}
					Some(MemEntry::Dir) => Err(FsError::FileExpected(format!(
						"path {} should be a file",
						py_repr(path)
					))),
					None => Err(FsError::NotFound(format!(
						"No such file or directory: {}",
						py_repr(path)
					))),
				}
			}
		}
	}

	/// `removedir(path)`: removes an empty directory.
	pub(crate) fn removedir(&self, path: &str) -> FsResult<()> {
		let c = self.components(path)?;
		match &self.backend {
			Backend::Os(root) => {
				let p = Self::os_path(root, &c);
				if p.is_file() {
					return Err(FsError::DirectoryExpected(format!(
						"path {} should be a directory",
						py_repr(path)
					)));
				}
				if std::fs::read_dir(&p).is_ok_and(|mut d| d.next().is_some()) {
					return Err(FsError::DirectoryNotEmpty(format!(
						"Directory not empty: {}",
						py_repr(path)
					)));
				}
				std::fs::remove_dir(&p).map_err(|e| match e.kind() {
					std::io::ErrorKind::NotFound => {
						FsError::NotFound(format!("No such file or directory: {}", py_repr(path)))
					}
					_ => FsError::Io(e),
				})
			}
			Backend::Mem(m) => {
				let mut m = lock(m);
				if m.read_only {
					return Err(FsError::ReadOnly(path.to_owned()));
				}
				let key = Self::key(&c);
				match m.entries.get(&key) {
					Some(MemEntry::Dir) => {
						let child = format!("{key}/");
						if m.entries.keys().any(|k| k.starts_with(&child)) {
							return Err(FsError::DirectoryNotEmpty(format!(
								"Directory not empty: {}",
								py_repr(path)
							)));
						}
						m.entries.remove(&key);
						Ok(())
					}
					Some(MemEntry::File(..)) => Err(FsError::DirectoryExpected(format!(
						"path {} should be a directory",
						py_repr(path)
					))),
					None => Err(FsError::NotFound(format!(
						"No such file or directory: {}",
						py_repr(path)
					))),
				}
			}
		}
	}

	/// `removetree(path)`: removes a directory and its contents.
	pub(crate) fn removetree(&self, path: &str) -> FsResult<()> {
		let c = self.components(path)?;
		match &self.backend {
			Backend::Os(root) => {
				std::fs::remove_dir_all(Self::os_path(root, &c)).map_err(|e| match e.kind() {
					std::io::ErrorKind::NotFound => {
						FsError::NotFound(format!("No such file or directory: {}", py_repr(path)))
					}
					_ => FsError::Io(e),
				})
			}
			Backend::Mem(m) => {
				let mut m = lock(m);
				if m.read_only {
					return Err(FsError::ReadOnly(path.to_owned()));
				}
				let key = Self::key(&c);
				if !m.entries.contains_key(&key) {
					return Err(FsError::NotFound(format!(
						"No such file or directory: {}",
						py_repr(path)
					)));
				}
				let child = format!("{key}/");
				m.entries.retain(|k, _| *k != key && !k.starts_with(&child));
				Ok(())
			}
		}
	}

	/// `makedir(path, recreate)`: creates a directory whose parent exists, and
	/// returns it.
	pub(crate) fn makedir(&self, path: &str, recreate: bool) -> FsResult<Fs> {
		self.mkdir(path, false, recreate)
	}

	/// `makedirs(path, recreate)`: creates a directory and its parents, and
	/// returns it.
	pub(crate) fn makedirs(&self, path: &str, recreate: bool) -> FsResult<Fs> {
		self.mkdir(path, true, recreate)
	}

	fn mkdir(&self, path: &str, parents: bool, exist_ok: bool) -> FsResult<Fs> {
		let c = self.components(path)?;
		match &self.backend {
			Backend::Os(root) => {
				let p = Self::os_path(root, &c);
				let result = if parents {
					if p.is_dir() && !exist_ok {
						Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
					} else {
						std::fs::create_dir_all(&p)
					}
				} else {
					std::fs::create_dir(&p)
				};
				match result {
					Ok(()) => {}
					Err(e)
						if e.kind() == std::io::ErrorKind::AlreadyExists
							&& exist_ok && p.is_dir() => {}
					Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
						return Err(FsError::Exists(format!("{} already exists", py_repr(path))));
					}
					Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
						return Err(FsError::NotFound(format!(
							"No such file or directory: {}",
							py_repr(path)
						)));
					}
					Err(e) => return Err(FsError::Io(e)),
				}
			}
			Backend::Mem(m) => {
				let mut m = lock(m);
				if m.read_only {
					return Err(FsError::ReadOnly(path.to_owned()));
				}
				for i in 1..=c.len() {
					let key = Self::key(&c[..i]);
					match m.entries.get(&key) {
						Some(MemEntry::Dir) => {
							if i == c.len() && !exist_ok {
								return Err(FsError::Exists(format!(
									"{} already exists",
									py_repr(path)
								)));
							}
						}
						Some(MemEntry::File(..)) => {
							return Err(FsError::DirectoryExpected(format!(
								"{} is not a directory",
								py_repr(path)
							)));
						}
						None => {
							if i < c.len() && !parents {
								return Err(FsError::NotFound(format!(
									"No such file or directory: {}",
									py_repr(path)
								)));
							}
							m.entries.insert(key, MemEntry::Dir);
						}
					}
				}
			}
		}
		self.opendir(path)
	}

	/// `opendir(path)`: the subdirectory as a file system (`SubFS`).
	pub(crate) fn opendir(&self, path: &str) -> FsResult<Fs> {
		if !self.exists(path)? {
			return Err(FsError::NotFound(format!(
				"No such file or directory: {}",
				py_repr(path)
			)));
		}
		if !self.isdir(path)? {
			return Err(FsError::DirectoryExpected(format!(
				"{} is not a directory",
				py_repr(path)
			)));
		}
		Ok(Fs {
			backend: self.backend.clone(),
			prefix: self.components(path)?,
			resolved_root: self.resolved_root.clone(),
		})
	}

	/// `scandir(path)`: the names of the entries of a directory, and whether
	/// they are directories, in listing order.
	pub(crate) fn scandir(&self, path: &str) -> FsResult<Vec<(String, bool)>> {
		let c = self.components(path)?;
		match &self.backend {
			Backend::Os(root) => {
				let p = Self::os_path(root, &c);
				let rd = std::fs::read_dir(&p).map_err(|e| match e.kind() {
					std::io::ErrorKind::NotFound => {
						FsError::NotFound(format!("No such file or directory: {}", py_repr(path)))
					}
					_ => FsError::Io(e),
				})?;
				let mut out = Vec::new();
				for entry in rd {
					let entry = entry.map_err(FsError::Io)?;
					let is_dir = entry.path().is_dir();
					out.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
				}
				Ok(out)
			}
			Backend::Mem(m) => {
				let m = lock(m);
				let key = Self::key(&c);
				if !key.is_empty() && !matches!(m.entries.get(&key), Some(MemEntry::Dir)) {
					return Err(FsError::NotFound(format!(
						"No such file or directory: {}",
						py_repr(path)
					)));
				}
				let start = if key.is_empty() {
					String::new()
				} else {
					format!("{key}/")
				};
				Ok(m.entries
					.iter()
					.filter_map(|(k, e)| {
						let rest = k.strip_prefix(&start)?;
						(!rest.is_empty() && !rest.contains('/'))
							.then(|| (rest.to_owned(), matches!(e, MemEntry::Dir)))
					})
					.collect())
			}
		}
	}

	/// `walk.files(path)`: the paths of all files below a directory, breadth
	/// first, starting with `/`.
	pub(crate) fn walk_files(&self, path: &str) -> FsResult<Vec<String>> {
		Ok(self
			.walk(path)?
			.into_iter()
			.filter(|(_, d)| !d)
			.map(|(p, _)| p)
			.collect())
	}

	/// `walk.dirs(path)`.
	pub(crate) fn walk_dirs(&self, path: &str) -> FsResult<Vec<String>> {
		Ok(self
			.walk(path)?
			.into_iter()
			.filter(|(_, d)| *d)
			.map(|(p, _)| p)
			.collect())
	}

	fn walk(&self, path: &str) -> FsResult<Vec<(String, bool)>> {
		let mut out = Vec::new();
		let mut queue = std::collections::VecDeque::from([path.to_owned()]);
		while let Some(dir_path) = queue.pop_front() {
			for (name, is_dir) in self.scandir(&dir_path)? {
				let p = combine(&dir_path, &name);
				if is_dir {
					queue.push_back(p.clone());
				}
				out.push((p, is_dir));
			}
		}
		Ok(out)
	}

	/// `movedir(src, dst, create=True)`.
	pub(crate) fn movedir(&self, src: &str, dst: &str) -> FsResult<()> {
		let sc = self.components(src)?;
		let dc = self.components(dst)?;
		if dc.starts_with(&sc) {
			return Err(FsError::Exists(format!(
				"cannot move {} to {}",
				py_repr(src),
				py_repr(dst)
			)));
		}
		match &self.backend {
			Backend::Os(root) => {
				let sp = Self::os_path(root, &sc);
				let dp = Self::os_path(root, &dc);
				if !sp.exists() {
					return Err(FsError::NotFound(format!(
						"Source {} does not exist",
						py_repr(src)
					)));
				}
				if !sp.is_dir() {
					return Err(FsError::DirectoryExpected(format!(
						"Source {} should be a directory",
						py_repr(src)
					)));
				}
				if dp.is_file() {
					return Err(FsError::DirectoryExpected(format!(
						"Destination {} should be a directory",
						py_repr(dst)
					)));
				}
				if let Some(parent) = dp.parent() {
					std::fs::create_dir_all(parent).map_err(FsError::Io)?;
				}
				if dp.exists() {
					if std::fs::read_dir(&dp).is_ok_and(|mut d| d.next().is_some()) {
						return Err(FsError::DirectoryNotEmpty(format!(
							"Destination {} is not empty",
							py_repr(dst)
						)));
					}
					std::fs::remove_dir(&dp).map_err(FsError::Io)?;
				}
				std::fs::rename(&sp, &dp).map_err(FsError::Io)
			}
			Backend::Mem(m) => {
				let mut m = lock(m);
				if m.read_only {
					return Err(FsError::ReadOnly(src.to_owned()));
				}
				let sk = Self::key(&sc);
				let dk = Self::key(&dc);
				match m.entries.get(&sk) {
					Some(MemEntry::Dir) => {}
					Some(_) => {
						return Err(FsError::DirectoryExpected(format!(
							"Source {} should be a directory",
							py_repr(src)
						)));
					}
					None => {
						return Err(FsError::NotFound(format!(
							"Source {} does not exist",
							py_repr(src)
						)));
					}
				}
				let dchild = format!("{dk}/");
				match m.entries.get(&dk) {
					Some(MemEntry::File(..)) => {
						return Err(FsError::DirectoryExpected(format!(
							"Destination {} should be a directory",
							py_repr(dst)
						)));
					}
					Some(MemEntry::Dir) if m.entries.keys().any(|k| k.starts_with(&dchild)) => {
						return Err(FsError::DirectoryNotEmpty(format!(
							"Destination {} is not empty",
							py_repr(dst)
						)));
					}
					_ => {}
				}
				for i in 1..dc.len() {
					m.entries
						.entry(Self::key(&dc[..i]))
						.or_insert(MemEntry::Dir);
				}
				let schild = format!("{sk}/");
				let moved: Vec<(String, MemEntry)> = m
					.entries
					.iter()
					.filter(|(k, _)| **k == sk || k.starts_with(&schild))
					.map(|(k, e)| (k.clone(), e.clone()))
					.collect();
				for (k, e) in moved {
					m.entries.remove(&k);
					let new_key = format!("{dk}{}", &k[sk.len()..]);
					m.entries.insert(new_key, e);
				}
				Ok(())
			}
		}
	}

	/// The modification time of a file (`getinfo(path, ["details"]).modified`),
	/// or `None` if it does not exist.
	pub(crate) fn modified(&self, path: &str) -> FsResult<Option<f64>> {
		let c = self.components(path)?;
		Ok(match &self.backend {
			Backend::Os(root) => std::fs::metadata(Self::os_path(root, &c))
				.ok()
				.and_then(|m| m.modified().ok())
				.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
				.map(|d| d.as_secs_f64()),
			Backend::Mem(m) => match lock(m).entries.get(&Self::key(&c)) {
				Some(MemEntry::File(_, t)) => *t,
				_ => None,
			},
		})
	}

	/// Copies a file or a directory tree from another file system
	/// (`fs.copy.copy_file`, `fs.copy.copy_dir`).
	pub(crate) fn copy_from(&self, src: &Fs, src_path: &str, dst_path: &str) -> FsResult<()> {
		if src.isdir(src_path)? {
			self.makedirs(dst_path, true)?;
			for dir in src.walk_dirs(src_path)? {
				let rel = &dir[src_path.trim_end_matches('/').len()..];
				self.makedir(&combine(dst_path, rel), true)?;
			}
			for file in src.walk_files(src_path)? {
				let rel = &file[src_path.trim_end_matches('/').len()..];
				self.writebytes(&combine(dst_path, rel), &src.readbytes(&file)?)?;
			}
			Ok(())
		} else {
			self.writebytes(dst_path, &src.readbytes(src_path)?)
		}
	}
}

impl fmt::Display for Fs {
	/// `str(fs)`: `<osfs 'C:\path'>`, followed by `/prefix` for a
	/// subdirectory.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self.backend {
			Backend::Os(root) => write!(f, "<osfs '{}'>", root.display())?,
			Backend::Mem(m) => f.write_str(&lock(m).label)?,
		}
		if !self.prefix.is_empty() {
			write!(f, "/{}", self.prefix.join("/"))?;
		}
		Ok(())
	}
}

/// `fs.path.combine`.
pub(crate) fn combine(path1: &str, path2: &str) -> String {
	if path1.is_empty() {
		return path2.to_owned();
	}
	format!(
		"{}/{}",
		path1.trim_end_matches('/'),
		path2.trim_start_matches('/')
	)
}

/// `fs.path.dirname`.
pub(crate) fn dirname(path: &str) -> String {
	match path.rsplit_once('/') {
		None => String::new(),
		Some(("", _)) => "/".into(),
		Some((d, _)) => d.to_owned(),
	}
}

/// `fs.path.basename`.
pub(crate) fn basename(path: &str) -> String {
	match path.rsplit_once('/') {
		None => path.to_owned(),
		Some((_, b)) => b.to_owned(),
	}
}

/// `repr(str)`.
pub(crate) fn py_repr(s: &str) -> String {
	fontoxide_misc::py::str_repr(s)
}

/// Removes `.` and `..` components without touching the disk.
/// The most symbolic links [`resolve_under`] follows, as `os.path.realpath`
/// (`_MAXLINKS`, beyond which the rest of the path is kept as is).
const MAX_LINKS: usize = 40;

/// `(base / rel).resolve()` for a `base` that is already resolved: symbolic
/// links are expanded as far as the path exists (`Path.resolve()`, not
/// strict). As in `os.path.realpath`, `..` in `rel` applies after the links
/// before it are expanded on POSIX, but on Windows, `rel` is normalized first
/// (`ntpath.realpath` calls `normpath`).
fn resolve_under<'a>(base: &Path, rel: impl IntoIterator<Item = &'a OsStr>) -> PathBuf {
	let mut out = base.to_path_buf();
	let mut pending: VecDeque<OsString> = VecDeque::new();
	for part in rel {
		if cfg!(windows) && part == ".." {
			if pending.pop_back().is_none() {
				out.pop();
			}
		} else if !part.is_empty() && part != "." {
			pending.push_back(part.to_owned());
		}
	}
	let mut links = 0;
	while let Some(part) = pending.pop_front() {
		if part.is_empty() || part == "." {
			continue;
		}
		if part == ".." {
			out.pop();
			continue;
		}
		out.push(&part);
		if links >= MAX_LINKS {
			continue;
		}
		let is_link = std::fs::symlink_metadata(&out).is_ok_and(|m| m.file_type().is_symlink());
		if !is_link {
			continue;
		}
		let Ok(target) = std::fs::read_link(&out) else {
			continue;
		};
		links += 1;
		out.pop();
		let target = strip_verbatim(&target);
		if target.has_root() || target.is_absolute() {
			out = PathBuf::new();
		}
		for c in target.components().rev() {
			pending.push_front(c.as_os_str().to_owned());
		}
	}
	out
}

/// A path without the `\?\` prefix of Windows verbatim paths, as
/// `os.path.realpath` gives it.
fn strip_verbatim(p: &Path) -> PathBuf {
	let s = p.to_string_lossy();
	if let Some(rest) = s.strip_prefix(r"\?\UNC\") {
		PathBuf::from(format!(r"\{rest}"))
	} else if let Some(rest) = s.strip_prefix(r"\?\") {
		PathBuf::from(rest)
	} else {
		p.to_path_buf()
	}
}

/// `path.is_relative_to(root)`, ignoring case on Windows as `PureWindowsPath`
/// does.
fn is_relative_to(path: &Path, root: &Path) -> bool {
	let key = |p: &Path| -> Vec<String> {
		p.components()
			.map(|c| {
				let s = c.as_os_str().to_string_lossy().into_owned();
				if cfg!(windows) { s.to_lowercase() } else { s }
			})
			.collect()
	};
	let (path, root) = (key(path), key(root));
	path.len() >= root.len() && path[..root.len()] == root[..]
}

fn normalize_os_path(p: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for c in p.components() {
		match c {
			Component::ParentDir => {
				out.pop();
			}
			Component::CurDir => {}
			other => out.push(other.as_os_str()),
		}
	}
	out
}
