//! Port of `fontTools.misc.cliTools`: utilities for command-line interfaces.

const SEPARATORS: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };

/// `os.path.split`, for paths without a Windows drive.
fn split(path: &str) -> (&str, &str) {
	let Some(at) = path.rfind(SEPARATORS) else {
		return ("", path);
	};
	let (head, tail) = path.split_at(at + 1);
	let stripped = head.trim_end_matches(SEPARATORS);
	(if stripped.is_empty() { head } else { stripped }, tail)
}

/// `os.path.splitext` of a file name: leading periods don't start an extension.
fn splitext(name: &str) -> (&str, &str) {
	let leading = name.len() - name.trim_start_matches('.').len();
	match name[leading..].rfind('.') {
		Some(at) => name.split_at(leading + at),
		None => (name, ""),
	}
}

/// `os.path.join` of a directory and a file name.
fn join(dir: &str, name: &str) -> String {
	if dir.is_empty() || dir.ends_with(SEPARATORS) {
		format!("{dir}{name}")
	} else {
		format!("{dir}{}{name}", SEPARATORS[SEPARATORS.len() - 1])
	}
}

/// Removes a number added to avoid overwriting a file (`#\d+$`).
fn strip_number(name: &str) -> &str {
	if let Some(at) = name.rfind('#') {
		let digits = &name[at + 1..];
		if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
			return &name[..at];
		}
	}
	name
}

/// `makeOutputFileName`: a file name to write the output for an input file to,
/// with the directory replaced by `output_dir`, `suffix` added to the name, and
/// the extension replaced by `extension`. Without `over_write`, a number is added
/// to avoid overwriting a file.
pub fn make_output_file_name(
	input: &str,
	output_dir: Option<&str>,
	extension: Option<&str>,
	over_write: bool,
	suffix: &str,
) -> String {
	let (dir, file) = split(input);
	let (name, input_extension) = splitext(file);
	let dir = output_dir.filter(|d| !d.is_empty()).unwrap_or(dir);
	let name = strip_number(name);
	let extension = extension.unwrap_or(input_extension);
	let mut output = join(dir, &format!("{name}{suffix}{extension}"));
	if !over_write {
		let mut n = 1;
		while std::path::Path::new(&output).exists() {
			output = join(dir, &format!("{name}{suffix}#{n}{extension}"));
			n += 1;
		}
	}
	output
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn output_file_names() {
		let name =
			|input, extension, suffix| make_output_file_name(input, None, extension, true, suffix);
		assert_eq!(name("font.ttf", None, ""), "font.ttf");
		assert_eq!(
			name("font.ttf", Some(".woff"), ".subset"),
			"font.subset.woff"
		);
		assert_eq!(
			name("a/font.otf", None, ".subset"),
			join("a", "font.subset.otf")
		);
		assert_eq!(name("font#12.ttf", None, ""), "font.ttf");
		assert_eq!(name("font#x.ttf", None, ""), "font#x.ttf");
		assert_eq!(name(".hidden", None, "-s"), ".hidden-s");
		assert_eq!(name("a.b.c", None, "-s"), "a.b-s.c");
		assert_eq!(
			make_output_file_name("a/font.ttf", Some("out"), None, true, ""),
			join("out", "font.ttf")
		);
	}
}
