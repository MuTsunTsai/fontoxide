"""Generates the fixtures of crates/fontoxide-diff/tests/diff.rs.

fontTools has no tests for `fontTools.diff`. This script runs its command line
(`fontTools.diff.run`) on fonts of fontTools' tests with various options, and
saves, for each case, what it writes to standard output and standard error and
its exit code, in crates/fontoxide-diff/tests/data/<case>.txt:

    <standard output>
    == stderr
    <standard error>
    == exit <code>

or `== exit error <exception>` for an exception that `run` lets through.

The font paths and the file modification times depend on the machine: they are
replaced with `<file1>`, `<file2>` and `<mtime1>`, `<mtime2>` (as the test does).
Arguments starting with `Tests/` are fonts of fontTools' tests.

    uv run --project tools/reference python tools/reference/diff_fixtures.py
"""

import contextlib
import io
import os
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools import diff
from fontTools.diff.utils import get_file_modtime

TESTS = ROOT / "upstream" / "fonttools" / "Tests"
OUT = ROOT / "crates" / "fontoxide-diff" / "tests" / "data"

TTF = "Tests/ttx/data/TestTTF.ttf"
OTF = "Tests/ttx/data/TestOTF.otf"
TTC = "Tests/ttx/data/TestTTC.ttc"
TTC2 = "Tests/ttx/data/TestTTCv2.ttc"
WOFF = "Tests/ttx/data/TestWOFF.woff"
WOFF2 = "Tests/ttx/data/TestWOFF2.woff2"
I_TTF = "Tests/ttLib/data/I.ttf"
I_OTF = "Tests/ttLib/data/I.otf"
VARC1 = "Tests/ttLib/data/varc-ac00-ac01.ttf"
VARC2 = "Tests/ttLib/data/varc-6868.ttf"

# Without `--color never`, the output is colored (see the UPSTREAM-BUG in the
# Rust port), so most cases do not color.
CASES = [
	("ttf_otf", [TTF, OTF, "--color", "never"]),
	("ttf_otf_color", [TTF, OTF]),
	("ttf_otf_color_always", [TTF, OTF, "--color", "always", "-U", "1"]),
	("ttf_otf_summary", [TTF, OTF, "-l"]),
	("ttf_otf_summary_include", [TTF, OTF, "-l", "-t", "head", "CFF", "glyf", "zzzz"]),
	("ttf_otf_summary_exclude", [TTF, OTF, "-l", "-x", "head", "CFF", "glyf"]),
	("ttf_otf_include", [TTF, OTF, "--color", "never", "-t", "head", "hhea", "GlyphOrder"]),
	("ttf_otf_exclude", [TTF, OTF, "--color", "never", "-x", "glyf", "CFF", "-U", "0"]),
	("ttf_otf_always", [TTF, OTF, "--color", "never", "-a", "-t", "maxp", "name"]),
	("ttf_otf_context", [TTF, OTF, "--color", "never", "--lines", "7"]),
	("ttf_otf_negative_context", [TTF, OTF, "--color", "never", "-U", "-2"]),
	("ttf_otf_quiet", [TTF, OTF, "-q"]),
	("ttf_otf_long_tags", [TTF, OTF, "--color", "never", "-t", "toolong", "zzzz"]),
	("ttf_otf_same_included", [TTF, OTF, "--color", "never", "-t", "OS/2", "post"]),
	("ttf_same", [TTF, TTF, "--color", "never"]),
	("ttf_same_summary", [TTF, TTF, "-l"]),
	("ttf_same_always", [TTF, TTF, "--color", "never", "-a", "-t", "head"]),
	("ttf_same_negative_context", [TTF, TTF, "--color", "never", "-U", "-1"]),
	("ttc_fonts", [TTC, TTC, "--color", "never", "--y1", "0", "--y2", "1"]),
	("ttc_summary", [TTC, TTC2, "-l", "--y1", "1", "--y2", "0"]),
	("ttc_no_number", [TTC, TTF, "--color", "never"]),
	("ttc_no_number_summary", [TTC, TTF, "-l"]),
	("ttc_bad_number_summary", [TTC, TTF, "-l", "--y1", "7"]),
	("woff_woff2", [WOFF, WOFF2, "--color", "never"]),
	("woff_woff2_summary", [WOFF, WOFF2, "-l"]),
	("woff2_ttf", [WOFF2, TTF, "--color", "never", "-a", "-U", "2"]),
	("I_ttf_otf", [I_TTF, I_OTF, "--color", "never"]),
	("varc", [VARC1, VARC2, "--color", "never", "-x", "glyf"]),
	("always_and_binary", [TTF, OTF, "-a", "-b"]),
	("always_and_binary_quiet", [TTF, OTF, "-a", "-b", "-q"]),
	("missing_file1", ["nonexistent.ttf", OTF]),
	("missing_file2", [TTF, "nonexistent.otf", "-q"]),
	("missing_diff_tool", [TTF, OTF, "--diff", "nonexistent-diff-tool"]),
	("empty_diff_tool", [TTF, OTF, "--diff", "", "--color", "never", "-t", "head"]),
	("bad_color", [TTF, OTF, "--color", "red"]),
	("bad_lines", [TTF, OTF, "-U", "x"]),
	("missing_files", [TTF]),
	("include_eats_files", ["-t", "head", TTF, OTF]),
	("help", ["-h"]),
]


def resolve(arg):
	if arg.startswith("Tests/"):
		return str(TESTS / arg[len("Tests/"):])
	return arg


def run_case(args):
	# `--diff=` uses Python's difflib even if there is a `diff` on the PATH.
	if "--diff" not in args and "-h" not in args:
		args = args + ["--diff="]
	args = [resolve(a) for a in args]
	stdout, stderr = io.StringIO(), io.StringIO()
	with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
		try:
			code = diff.run(args)
		except SystemExit as e:
			code = e.code
		except Exception as e:
			# Uncaught: a traceback.
			code = f"error {type(e).__name__}: {e}"
	out, err = stdout.getvalue(), stderr.getvalue().replace("\r\n", "\n")
	files = [a for a in args if os.path.isfile(a)]
	for n, f in enumerate(files[:2], 1):
		mtime = get_file_modtime(f)
		out = out.replace(f"\t{mtime}\n", f"\t<mtime{n}>\n")
	for n, f in enumerate(files[:2], 1):
		out = out.replace(f, f"<file{n}>")
		err = err.replace(f, f"<file{n}>")
	return f"{out}== stderr\n{err}== exit {code}\n"


def main():
	# The `prog` of the parser.
	sys.argv[0] = "fonttools diff"
	OUT.mkdir(parents=True, exist_ok=True)
	for name, args in CASES:
		result = run_case(args)
		(OUT / f"{name}.txt").write_text(result, encoding="utf-8", newline="\n")
		print(name)


if __name__ == "__main__":
	main()
