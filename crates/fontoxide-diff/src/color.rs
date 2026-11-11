//! Port of `fontTools.diff.color`: coloring the lines of a unified diff with
//! ANSI escape codes.

/// `ansicolors`: the ANSI escape codes by name.
pub const ANSICOLORS: &[(&str, &str)] = &[
	("BLACK", "\x1b[30m"),
	("RED", "\x1b[31m"),
	("GREEN", "\x1b[32m"),
	("YELLOW", "\x1b[33m"),
	("BLUE", "\x1b[34m"),
	("MAGENTA", "\x1b[35m"),
	("CYAN", "\x1b[36m"),
	("WHITE", "\x1b[37m"),
	("BOLD", "\x1b[1m"),
	("RESET", "\x1b[0m"),
];

/// `green_start`.
pub const GREEN_START: &str = "\x1b[32m";
/// `red_start`.
pub const RED_START: &str = "\x1b[31m";
/// `cyan_start`.
pub const CYAN_START: &str = "\x1b[36m";
/// `reset`.
pub const RESET: &str = "\x1b[0m";

/// `color_unified_diff_line`: the line colored according to its type in a
/// unified diff: added lines green, removed lines red, hunk headers cyan.
pub fn color_unified_diff_line(line: &str) -> String {
	let start = |n: usize| line.get(..n).unwrap_or(line);
	let color = if start(2) == "+ " || line == "+\n" {
		// A lone "+" is an added empty line.
		GREEN_START
	} else if start(2) == "- " || line == "-\n" {
		// A lone "-" is a deleted empty line.
		RED_START
	} else if start(3) == "@@ " {
		CYAN_START
	} else if start(4) == "--- " {
		RED_START
	} else if start(4) == "+++ " {
		GREEN_START
	} else {
		return line.to_owned();
	};
	format!("{color}{line}{RESET}")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn colors() {
		assert_eq!(color_unified_diff_line("+ a\n"), "\x1b[32m+ a\n\x1b[0m");
		assert_eq!(color_unified_diff_line("-\n"), "\x1b[31m-\n\x1b[0m");
		assert_eq!(
			color_unified_diff_line("@@ -1 +1 @@\n"),
			"\x1b[36m@@ -1 +1 @@\n\x1b[0m"
		);
		assert_eq!(color_unified_diff_line("--- a\n"), "\x1b[31m--- a\n\x1b[0m");
		assert_eq!(color_unified_diff_line("+++ b\n"), "\x1b[32m+++ b\n\x1b[0m");
		// Only lines whose text starts with a space are colored.
		assert_eq!(color_unified_diff_line("-<a/>\n"), "-<a/>\n");
		assert_eq!(color_unified_diff_line("  a\n"), "  a\n");
		assert_eq!(color_unified_diff_line("é\n"), "é\n");
	}
}
