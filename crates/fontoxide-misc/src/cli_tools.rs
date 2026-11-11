//! Port of `fontTools.misc.cliTools`: utilities for command-line interfaces.
//!
//! Also holds the program name of the command lines ([`with_program_name`],
//! [`program_text`]), with which the `fontoxide` executable names itself in their
//! usage and help texts.

use std::borrow::Cow;
use std::cell::Cell;

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

thread_local! {
	static PROGRAM_NAME: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// The name of the program running the command lines, when it is not
/// fontTools' (see [`with_program_name`]).
pub fn program_name() -> Option<&'static str> {
	PROGRAM_NAME.with(Cell::get)
}

/// Runs `f` with the command lines naming themselves `<name> <module>` in their
/// usage and help texts (see [`program_text`]), on this thread. This is how the
/// `fontoxide` executable runs a module; by default, the texts are fontTools'.
pub fn with_program_name<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
	struct Restore(Option<&'static str>);
	impl Drop for Restore {
		fn drop(&mut self) {
			PROGRAM_NAME.with(|p| p.set(self.0));
		}
	}
	let _restore = Restore(PROGRAM_NAME.with(|p| p.replace(Some(name))));
	f()
}

/// A usage, help or error text of the command line of `module` (e.g.
/// `"subset"`), which names the program `prog` as fontTools does (e.g.
/// `"fonttools subset"`, or an `argparse` `prog` such as `"cu2qu"`), adapted to
/// the program name set by [`with_program_name`]: the program becomes
/// `"<name> <module>"`. Without a program name, the text is returned unchanged.
///
/// A `prog` of the form `fonttools <module>` is replaced wherever it appears as a
/// word, including the examples of the help. Another `prog` is only replaced
/// where it names the program for `argparse`: after `usage: ` at the start of a
/// line, and before `: error:` at the start of a line. The continuation lines of
/// a usage are indented to keep them aligned.
pub fn program_text<'a>(text: &'a str, prog: &str, module: &str) -> Cow<'a, str> {
	let Some(name) = program_name() else {
		return Cow::Borrowed(text);
	};
	let new = format!("{name} {module}");
	if new == prog {
		return Cow::Borrowed(text);
	}
	let everywhere = prog.starts_with("fonttools ");
	let delta = new.chars().count() as isize - prog.chars().count() as isize;
	let usage_prefix = format!("usage: {prog}");
	let mut out = String::with_capacity(text.len() + 64);
	let mut in_usage = false;
	for line in text.split_inclusive('\n') {
		if in_usage && line.starts_with(' ') && !line.trim().is_empty() {
			out.push_str(&reindent(line, delta));
			continue;
		}
		in_usage = false;
		if everywhere {
			if line.starts_with(&usage_prefix) && is_word_end(&line[usage_prefix.len()..]) {
				in_usage = true;
			}
			out.push_str(&replace_words(line, prog, &new));
		} else if let Some(rest) = line.strip_prefix(&usage_prefix)
			&& is_word_end(rest)
		{
			in_usage = true;
			out.push_str("usage: ");
			out.push_str(&new);
			out.push_str(rest);
		} else if let Some(rest) = line.strip_prefix(prog)
			&& rest.starts_with(": error:")
		{
			out.push_str(&new);
			out.push_str(rest);
		} else {
			out.push_str(line);
		}
	}
	Cow::Owned(out)
}

/// Whether `rest`, the text after a word, does not continue the word.
fn is_word_end(rest: &str) -> bool {
	!rest
		.chars()
		.next()
		.is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Replaces the occurrences of the words `old` in `line` with `new`.
fn replace_words(line: &str, old: &str, new: &str) -> String {
	let mut out = String::with_capacity(line.len());
	let mut rest = line;
	while let Some(at) = rest.find(old) {
		let before = rest[..at].chars().next_back();
		let starts_word =
			!before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '/'));
		let after = &rest[at + old.len()..];
		out.push_str(&rest[..at]);
		out.push_str(if starts_word && is_word_end(after) {
			new
		} else {
			old
		});
		rest = after;
	}
	out.push_str(rest);
	out
}

/// Moves a continuation line of a usage by `delta` columns, keeping at least
/// one space of indentation.
fn reindent(line: &str, delta: isize) -> Cow<'_, str> {
	if delta > 0 {
		Cow::Owned(format!("{}{line}", " ".repeat(delta.unsigned_abs())))
	} else {
		let spaces = line.len() - line.trim_start_matches(' ').len();
		let remove = delta.unsigned_abs().min(spaces.saturating_sub(1));
		Cow::Borrowed(&line[remove..])
	}
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

	#[test]
	fn program_texts() {
		let text = "usage: fonttools subset font [-h]\n\
		            Try fonttools subset --help; see github.com/fonttools/fonttools.\n\
		            $ fonttools subsetter fonttools subset\n";
		// fontTools' names by default.
		assert!(matches!(
			program_text(text, "fonttools subset", "subset"),
			Cow::Borrowed(t) if t == text
		));
		with_program_name("fontoxide", || {
			assert_eq!(
				program_text(text, "fonttools subset", "subset"),
				"usage: fontoxide subset font [-h]\n\
				 Try fontoxide subset --help; see github.com/fonttools/fonttools.\n\
				 $ fonttools subsetter fontoxide subset\n"
			);
			// Another `argparse` prog: only where argparse names the program.
			let text = "usage: cu2qu [-h] [-i |\n             -j [N]]\n\n  INPUT  cu2qu input\n\
			            cu2qu: error: oops";
			assert_eq!(
				program_text(text, "cu2qu", "cu2qu"),
				"usage: fontoxide cu2qu [-h] [-i |\n                       -j [N]]\n\n  INPUT  cu2qu input\n\
				 fontoxide cu2qu: error: oops"
			);
			// Nested programs restore the outer name.
			with_program_name("other", || assert_eq!(program_name(), Some("other")));
			assert_eq!(program_name(), Some("fontoxide"));
		});
		assert_eq!(program_name(), None);
	}
}
