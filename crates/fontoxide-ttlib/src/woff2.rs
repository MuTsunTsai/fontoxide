//! WOFF 2.0 (`fontTools.ttLib.woff2`).

use std::sync::Arc;

use fontoxide_misc::Tag;

use crate::sfnt::{DirectoryEntry, FlavorData, SfntReader, SfntReaderOptions, TableData};
use crate::{Error, Result};

/// The decompressed table data of a WOFF 2.0 file.
#[derive(Debug)]
pub struct Woff2Data {}

impl Woff2Data {
	pub(crate) fn table_data(
		&self,
		_entry: &DirectoryEntry,
		_reader: &SfntReader,
	) -> Result<TableData> {
		Err(Error::ttlib("WOFF2 is not implemented yet"))
	}
}

/// Reads a WOFF 2.0 file (`WOFF2Reader`).
pub(crate) fn read(_file: Arc<[u8]>, _options: SfntReaderOptions) -> Result<SfntReader> {
	Err(Error::ttlib("WOFF2 is not implemented yet"))
}

/// Writes a WOFF 2.0 file (`WOFF2Writer`).
#[derive(Debug)]
pub struct Woff2Writer {}

impl Woff2Writer {
	pub(crate) fn new(
		_num_tables: usize,
		_sfnt_version: Tag,
		_flavor_data: Option<FlavorData>,
	) -> Self {
		Woff2Writer {}
	}

	pub(crate) fn add(&mut self, _tag: Tag, _data: &[u8]) -> Result<()> {
		Err(Error::ttlib("WOFF2 is not implemented yet"))
	}

	pub(crate) fn close(self) -> Result<Vec<u8>> {
		Err(Error::ttlib("WOFF2 is not implemented yet"))
	}
}
