//! Port of `fontTools.misc.macRes`: a reader for Mac OS resource forks.

use std::path::Path;

use indexmap::IndexMap;
use thiserror::Error;

/// Errors of [`ResourceReader`] (`ResourceError`, plus the I/O and decoding
/// errors fontTools lets through).
#[derive(Debug, Error)]
pub enum ResourceError {
	/// `ResourceError`.
	#[error("{0}")]
	Resource(String),
	/// The file could not be read.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// Other errors (`ValueError`, decoding errors).
	#[error("{0}")]
	Other(String),
}

type Result<T> = std::result::Result<T, ResourceError>;

/// A resource stored within a resource fork (`Resource`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resource {
	/// The resource type (`type`).
	pub type_: String,
	/// The resource data (`data`).
	pub data: Vec<u8>,
	/// The resource ID (`id`).
	pub id: i16,
	/// The resource name, if it has one (`name`).
	pub name: Option<String>,
	/// The attributes (`attr`).
	pub attr: u8,
}

/// Reader for Mac OS resource forks (`ResourceReader`).
///
/// Resources are grouped by type, in the order of the type list of the fork.
#[derive(Debug, Clone, Default)]
pub struct ResourceReader {
	resources: IndexMap<String, Vec<Resource>>,
}

/// The fields of the header and map of a fork that the reader needs.
struct Header {
	data_offset: u64,
	abs_type_list_offset: u64,
	abs_name_list_offset: i64,
}

struct Reader<'a> {
	data: &'a [u8],
	pos: u64,
}

impl Reader<'_> {
	/// `_read`: `num_bytes` bytes at `offset`, or at the current position.
	fn read(&mut self, num_bytes: u64, offset: Option<i64>) -> Result<&[u8]> {
		if let Some(offset) = offset {
			// `BytesIO.seek` rejects negative positions with a `ValueError`.
			self.pos = u64::try_from(offset)
				.map_err(|_| ResourceError::Other(format!("negative seek value {offset}")))?;
		}
		let start = self.pos.min(self.data.len() as u64) as usize;
		let end = self
			.pos
			.saturating_add(num_bytes)
			.min(self.data.len() as u64) as usize;
		self.pos = self.pos.max(end as u64);
		if (end - start) as u64 != num_bytes {
			return Err(ResourceError::Resource(
				"Cannot read resource (not enough data)".into(),
			));
		}
		Ok(&self.data[start..end])
	}

	fn u16_at(&mut self, offset: i64) -> Result<u16> {
		let b = self.read(2, Some(offset))?;
		Ok(u16::from_be_bytes([b[0], b[1]]))
	}

	fn u32_at(&mut self, offset: i64) -> Result<u32> {
		let b = self.read(4, Some(offset))?;
		Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
	}
}

fn mac_roman(data: &[u8]) -> Result<String> {
	fontoxide_encodings::codecs::decode("mac-roman", data)
		.map_err(|e| ResourceError::Other(e.to_string()))
}

impl ResourceReader {
	/// Parses a resource fork from its data (`ResourceReader(fileobj)`).
	pub fn from_bytes(data: &[u8]) -> Result<Self> {
		let mut reader = Reader { data, pos: 0 };
		let header = Self::read_header_and_map(&mut reader)?;
		let resources = Self::read_type_list(&mut reader, &header)?;
		Ok(ResourceReader { resources })
	}

	/// Opens a file (`ResourceReader(path)`): its resource fork where the file
	/// system has them (macOS), otherwise the data fork, read as though it were a
	/// resource fork.
	pub fn open(path: impl AsRef<Path>) -> Result<Self> {
		let path = path.as_ref();
		let mut fork = path.as_os_str().to_owned();
		fork.push("/..namedfork/rsrc");
		if let Ok(data) = std::fs::read(&fork) {
			match Self::from_bytes(&data) {
				Ok(reader) => return Ok(reader),
				Err(ResourceError::Resource(_) | ResourceError::Io(_)) => {}
				Err(e) => return Err(e),
			}
		}
		let data = std::fs::read(path)?;
		Self::from_bytes(&data)
	}

	fn read_header_and_map(reader: &mut Reader<'_>) -> Result<Header> {
		let data_offset = reader.u32_at(0)? as u64;
		let map_offset = reader.u32_at(4)? as i64;
		reader.read(8, None)?;
		// Skip the reserved part of the map, then its attributes.
		let type_list_offset = reader.u16_at(map_offset + 24)? as i64;
		let name_list_offset = reader.u16_at(map_offset + 26)? as i64;
		Ok(Header {
			data_offset,
			abs_type_list_offset: (map_offset + type_list_offset) as u64,
			abs_name_list_offset: map_offset + name_list_offset,
		})
	}

	fn read_type_list(
		reader: &mut Reader<'_>,
		header: &Header,
	) -> Result<IndexMap<String, Vec<Resource>>> {
		let abs = header.abs_type_list_offset as i64;
		let num_types = reader.u16_at(abs)? as i64;
		let mut out = IndexMap::new();
		for i in 0..=num_types {
			let item = reader.read(8, Some(abs + 2 + 8 * i))?;
			let res_type = mac_roman(&item[..4])?;
			let num_res = u16::from_be_bytes([item[4], item[5]]) as i64 + 1;
			let ref_list_offset = abs + u16::from_be_bytes([item[6], item[7]]) as i64;
			let mut resources = Vec::new();
			for j in 0..num_res {
				let r = reader.read(12, Some(ref_list_offset + 12 * j))?.to_vec();
				resources.push(Self::read_resource(reader, header, res_type.clone(), &r)?);
			}
			out.insert(res_type, resources);
		}
		Ok(out)
	}

	/// `Resource.decompile`.
	fn read_resource(
		reader: &mut Reader<'_>,
		header: &Header,
		type_: String,
		r: &[u8],
	) -> Result<Resource> {
		let id = i16::from_be_bytes([r[0], r[1]]);
		let name_offset = i16::from_be_bytes([r[2], r[3]]);
		let attr = r[4];
		let data_offset = u32::from_be_bytes([0, r[5], r[6], r[7]]) as u64;
		let abs_data_offset = (header.data_offset + data_offset) as i64;
		let length = reader.u32_at(abs_data_offset)? as u64;
		let data = reader.read(length, None)?.to_vec();
		let mut name = None;
		if name_offset != -1 {
			let abs_name_offset = header.abs_name_list_offset + name_offset as i64;
			let len = reader.read(1, Some(abs_name_offset))?[0] as u64;
			name = Some(mac_roman(reader.read(len, None)?)?);
		}
		Ok(Resource {
			type_,
			data,
			id,
			name,
			attr,
		})
	}

	/// The resources of a type (`reader[resType]`).
	pub fn get(&self, res_type: &str) -> Option<&[Resource]> {
		self.resources.get(res_type).map(Vec::as_slice)
	}

	/// Sets the resources of a type (`reader[resType] = resources`).
	pub fn set(&mut self, res_type: String, resources: Vec<Resource>) {
		self.resources.insert(res_type, resources);
	}

	/// Removes the resources of a type (`del reader[resType]`).
	pub fn remove(&mut self, res_type: &str) -> Option<Vec<Resource>> {
		self.resources.shift_remove(res_type)
	}

	/// The number of types (`len(reader)`).
	pub fn len(&self) -> usize {
		self.resources.len()
	}

	/// Whether there are no types.
	pub fn is_empty(&self) -> bool {
		self.resources.is_empty()
	}

	/// The types of resources in the fork (`keys()`, `types`).
	pub fn types(&self) -> Vec<&str> {
		self.resources.keys().map(String::as_str).collect()
	}

	/// Iterates over the types and their resources.
	pub fn iter(&self) -> impl Iterator<Item = (&str, &[Resource])> {
		self.resources
			.iter()
			.map(|(k, v)| (k.as_str(), v.as_slice()))
	}

	/// The number of resources of a type (`countResources`).
	pub fn count_resources(&self, res_type: &str) -> usize {
		self.get(res_type).map_or(0, <[Resource]>::len)
	}

	/// The indices of the resources of a type, from 1 (`getIndices`).
	pub fn get_indices(&self, res_type: &str) -> Vec<usize> {
		(1..=self.count_resources(res_type)).collect()
	}

	/// The names of the resources of a type that have one (`getNames`).
	pub fn get_names(&self, res_type: &str) -> Vec<&str> {
		self.get(res_type)
			.unwrap_or_default()
			.iter()
			.filter_map(|r| r.name.as_deref())
			.collect()
	}

	/// The resource of a type at an index from 1 (`getIndResource`).
	pub fn get_ind_resource(&self, res_type: &str, index: i64) -> Option<&Resource> {
		if index < 1 {
			return None;
		}
		self.get(res_type)?.get(usize::try_from(index - 1).ok()?)
	}

	/// The resource of a type with a name (`getNamedResource`).
	pub fn get_named_resource(&self, res_type: &str, name: &str) -> Option<&Resource> {
		self.get(res_type)?
			.iter()
			.find(|r| r.name.as_deref() == Some(name))
	}
}
