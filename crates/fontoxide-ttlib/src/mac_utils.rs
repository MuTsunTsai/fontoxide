//! Port of `fontTools.ttLib.macUtils`: fonts in the `sfnt` resources of Mac
//! resource forks (`.dfont` files and font suitcases), and the
//! `res_name_or_index` argument of `TTFont` ([`Font::open`]).
//!
//! As in fontTools, a file's resource fork is read where the file system has
//! them (macOS); elsewhere, the data fork is read as a resource fork, which is
//! how `.dfont` files store their resources.

use std::path::Path;

use fontoxide_misc::mac_res::{ResourceError, ResourceReader};

use crate::{Error, Font, ReadOptions, Result};

/// Which `sfnt` resource to read (`res_name_or_index`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResNameOrIndex {
	/// The resource with this name.
	Name(String),
	/// The resource at this index, from 1. With [`Font::open`], 0 reads the
	/// first `sfnt` resource if the file has any, else the file itself.
	Index(i64),
}

fn resource_error(e: ResourceError) -> Error {
	match e {
		ResourceError::Io(e) => Error::Io(e),
		ResourceError::Resource(m) => Error::ttlib(format!("ResourceError: {m}")),
		ResourceError::Other(m) => Error::value(m),
	}
}

/// The indices, from 1, of the `sfnt` resources of a file, or none if it is
/// not a resource fork (`getSFNTResIndices`).
pub fn get_sfnt_res_indices(path: impl AsRef<Path>) -> Result<Vec<usize>> {
	match ResourceReader::open(path) {
		Ok(reader) => Ok(reader.get_indices("sfnt")),
		Err(ResourceError::Resource(_)) => Ok(Vec::new()),
		Err(e) => Err(resource_error(e)),
	}
}

/// The data of an `sfnt` resource of a resource fork.
pub fn sfnt_resource_data(reader: &ResourceReader, res: &ResNameOrIndex) -> Result<Vec<u8>> {
	let rsrc = match res {
		ResNameOrIndex::Name(name) => reader.get_named_resource("sfnt", name),
		ResNameOrIndex::Index(index) => reader.get_ind_resource("sfnt", *index),
	};
	match rsrc {
		Some(rsrc) => Ok(rsrc.data.clone()),
		None => Err(Error::ttlib(format!(
			"sfnt resource not found: {}",
			match res {
				ResNameOrIndex::Name(name) => name.clone(),
				ResNameOrIndex::Index(index) => index.to_string(),
			}
		))),
	}
}

/// The data of an `sfnt` resource of a file (`SFNTResourceReader(path,
/// res_name_or_index)`).
pub fn sfnt_resource(path: impl AsRef<Path>, res: &ResNameOrIndex) -> Result<Vec<u8>> {
	let reader = ResourceReader::open(path).map_err(resource_error)?;
	sfnt_resource_data(&reader, res)
}

/// The fonts of a file (`openTTFonts`): the file itself if it has no `sfnt`
/// resources, else one font per resource.
pub fn open_tt_fonts(path: impl AsRef<Path>) -> Result<Vec<Font>> {
	let path = path.as_ref();
	let sfnts = get_sfnt_res_indices(path)?;
	if sfnts.is_empty() {
		return Ok(vec![Font::open(path, None, ReadOptions::default())?]);
	}
	sfnts
		.into_iter()
		.map(|index| {
			Font::open(
				path,
				Some(&ResNameOrIndex::Index(index as i64)),
				ReadOptions::default(),
			)
		})
		.collect()
}

impl Font {
	/// Reads a font file (`TTFont(path, res_name_or_index)`). With
	/// `res_name_or_index`, the font is the `sfnt` resource of the file's
	/// resource fork that it names; [`ResNameOrIndex::Index`] 0 reads the first
	/// one if there is any, else the file as an ordinary font file, as `ttx` does.
	/// Without it, this is [`Font::read`] on the file's data.
	pub fn open(
		path: impl AsRef<Path>,
		res_name_or_index: Option<&ResNameOrIndex>,
		options: ReadOptions,
	) -> Result<Font> {
		let path = path.as_ref();
		let data = match res_name_or_index {
			None => std::fs::read(path)?,
			Some(ResNameOrIndex::Index(0)) => {
				if get_sfnt_res_indices(path)?.is_empty() {
					std::fs::read(path)?
				} else {
					sfnt_resource(path, &ResNameOrIndex::Index(1))?
				}
			}
			Some(res) => sfnt_resource(path, res)?,
		};
		Font::read(data, options)
	}
}
