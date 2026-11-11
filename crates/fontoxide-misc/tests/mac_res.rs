//! `Tests/misc/macRes_test.py`.

use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_misc::mac_res::{Resource, ResourceError, ResourceReader};
use fontoxide_misc::text_tools::de_hex_str;

// test resource data in DeRez notation
//
// data 'TEST' (128, "name1") { $"4865 6C6C 6F" };                   /* Hello */
// data 'TEST' (129, "name2") { $"576F 726C 64" };                   /* World */
// data 'test' (130, "name3") { $"486F 7720 6172 6520 796F 753F" };  /* How are you? */
//
// the same data, compiled using Rez
const TEST_RSRC_FORK: &str = concat!(
	"00 00 01 00 00 00 01 22 00 00 00 22 00 00 00 64 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 05 48 65 6c 6c 6f 00 00 00 05 57 6f 72 ",
	"6c 64 00 00 00 0c 48 6f 77 20 61 72 65 20 79 6f ",
	"75 3f 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ",
	"00 00 00 00 00 00 00 00 00 00 00 1c 00 52 00 01 ",
	"54 45 53 54 00 01 00 12 74 65 73 74 00 00 00 2a ",
	"00 80 00 00 00 00 00 00 00 00 00 00 00 81 00 06 ",
	"00 00 00 09 00 00 00 00 00 82 00 0c 00 00 00 12 ",
	"00 00 00 00 05 6e 61 6d 65 31 05 6e 61 6d 65 32 ",
	"05 6e 61 6d 65 33                               ",
);

fn fork() -> Vec<u8> {
	de_hex_str(TEST_RSRC_FORK).unwrap_or_default()
}

fn resources(reader: &ResourceReader) -> Vec<Resource> {
	reader.iter().flat_map(|(_, r)| r.iter().cloned()).collect()
}

fn assert_rez_equal(res: &Resource, type_: &str, data: &[u8], id: i16, name: &str) {
	assert_eq!(res.type_, type_);
	assert_eq!(res.data, data);
	assert_eq!(res.id, id);
	assert_eq!(res.name.as_deref(), Some(name));
}

fn assert_expected(resources: &[Resource]) {
	assert_rez_equal(&resources[0], "TEST", b"Hello", 128, "name1");
	assert_rez_equal(&resources[1], "TEST", b"World", 129, "name2");
	assert_rez_equal(&resources[2], "test", b"How are you?", 130, "name3");
}

#[test]
fn test_read_file() -> Result<(), ResourceError> {
	let reader = ResourceReader::from_bytes(&fork())?;
	assert_expected(&resources(&reader));
	assert_eq!(reader.types(), ["TEST", "test"]);
	assert_eq!(reader.count_resources("TEST"), 2);
	assert_eq!(reader.get_indices("TEST"), [1, 2]);
	assert_eq!(reader.get_names("TEST"), ["name1", "name2"]);
	assert_eq!(reader.get_ind_resource("TEST", 2).map(|r| r.id), Some(129));
	assert_eq!(
		reader.get_named_resource("test", "name3").map(|r| r.id),
		Some(130)
	);
	Ok(())
}

#[test]
fn test_read_datafork() -> Result<(), ResourceError> {
	static N: AtomicUsize = AtomicUsize::new(0);
	let path = std::env::temp_dir().join(format!(
		"fontoxide-macres-{}-{}",
		std::process::id(),
		N.fetch_add(1, Ordering::SeqCst)
	));
	std::fs::write(&path, fork())?;
	let reader = ResourceReader::open(&path);
	let _ = std::fs::remove_file(&path);
	assert_expected(&resources(&reader?));
	Ok(())
}

/// `test_read_namedfork_rsrc`: resource forks exist on macOS only.
#[cfg(target_os = "macos")]
#[test]
fn test_read_namedfork_rsrc() -> Result<(), ResourceError> {
	let path = std::env::temp_dir().join(format!("fontoxide-macres-fork-{}", std::process::id()));
	std::fs::write(&path, b"")?;
	let mut fork_path = path.as_os_str().to_owned();
	fork_path.push("/..namedfork/rsrc");
	std::fs::write(&fork_path, fork())?;
	let reader = ResourceReader::open(&path);
	let _ = std::fs::remove_file(&path);
	assert_expected(&resources(&reader?));
	Ok(())
}
