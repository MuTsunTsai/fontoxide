//! `_UFOBaseIO`: the plist reading and writing shared by the UFO reader and
//! writer and by glyph sets.

use fontoxide_misc::plistlib::{self, Value};

use crate::errors::{Result, UfoLibError};
use crate::fs::{Fs, FsError};

/// `getFileModificationTime`: the modification time of a file, in seconds
/// since the epoch, or `None` if it does not exist.
pub(crate) fn file_modification_time(fs: &Fs, path: &str) -> Result<Option<f64>> {
	match fs.modified(path) {
		Ok(t) => Ok(t),
		Err(FsError::NotFound(_)) => Ok(None),
		Err(e) => Err(e.into()),
	}
}

/// `_getPlist`: reads a plist. A missing file is an error if there is no
/// default.
pub(crate) fn get_plist(fs: &Fs, file_name: &str, default: Option<Value>) -> Result<Value> {
	let data = match fs.readbytes(file_name) {
		Ok(data) => data,
		Err(FsError::NotFound(_)) => {
			return match default {
				Some(d) => Ok(d),
				None => Err(UfoLibError::UfoLib(format!(
					"'{file_name}' is missing on {fs}. This file is required"
				))),
			};
		}
		Err(e) => {
			return Err(UfoLibError::UfoLib(format!(
				"'{file_name}' could not be read on {fs}: {e}"
			)));
		}
	};
	plistlib::loads_bytes(&data)
		.map_err(|e| UfoLibError::UfoLib(format!("'{file_name}' could not be read on {fs}: {e}")))
}

/// `_writePlist`: writes a plist, unless an existing file of a UFO that
/// existed before already has the same data (so that its modification time
/// is kept).
pub(crate) fn write_plist(
	fs: &Fs,
	have_previous_file: bool,
	file_name: &str,
	obj: &Value,
) -> Result<()> {
	let data = match plistlib::dumps(obj, true, true) {
		Ok(data) => data.into_bytes(),
		Err(e) => {
			if !have_previous_file {
				// fontTools opens the file before serializing.
				fs.writebytes(file_name, b"")?;
			}
			return Err(UfoLibError::UfoLib(format!(
				"'{file_name}' could not be written on {fs} because the data is not properly formatted: {e}"
			)));
		}
	};
	if have_previous_file && fs.exists(file_name)? && fs.readbytes(file_name)? == data {
		return Ok(());
	}
	fs.writebytes(file_name, &data)?;
	Ok(())
}
