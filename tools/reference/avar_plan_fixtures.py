"""Generates the fixtures of crates/fontoxide-varlib/tests/avar_plan.rs.

fontTools has no tests for `fontTools.varLib.avar.plan`. This script runs its
`main` on fonts of fontTools' tests with various options, and saves, for each
case, what it prints and the `avar` table of the font it saves, in
crates/fontoxide-varlib/tests/data/avar_plan/<case>.txt:

    <the designspace snippets printed to standard output>
    == avar
    <the hexadecimal data of the avar table, or "none">

A case that fails has `== error` and the exception instead. The fonts given as
TTX are compiled first (as the test does).

    uv run --project tools/reference python tools/reference/avar_plan_fixtures.py
"""

import contextlib
import io
import os
import pathlib
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

import fontTools
from fontTools.ttLib import TTFont
from fontTools.varLib.avar import plan

TESTS = ROOT / "upstream" / "fonttools" / "Tests"
OUT = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "avar_plan"

# (case name, font relative to Tests/, arguments)
CASES = [
	("I_ttf", "ttLib/data/I.ttf", []),
	("I_ttf_sanitize", "ttLib/data/I.ttf", ["-s"]),
	(
		"I_ttf_values",
		"ttLib/data/I.ttf",
		[
			"--samples", "3",
			"--weights", "150 333.5 600 999",
			"--widths", "50 75 125 150",
			"--slants", "-5 -2.5 -9",
			"--sizes", "10 12 72",
		],
	),
	(
		"I_ttf_pins",
		"ttLib/data/I.ttf",
		[
			"--weight-pins", "250:300 700:650",
			"--weight-design-limits", "1:4:10",
			"--width-pins", "50:60",
			"--slant-design-limits=-12:0:0",
			"--optical-size-pins", "20:18 72:80",
		],
	),
	("I_ttf_option_like_value", "ttLib/data/I.ttf", ["--slant-design-limits", "-12:0:0"]),
	("I_ttf_glyphs", "ttLib/data/I.ttf", ["-g", "I .notdef I"]),
	("I_ttf_frequencies", "ttLib/data/I.ttf", ["-g", "I:2.5 .notdef:0", "-s"]),
	("I_ttf_zero_samples", "ttLib/data/I.ttf", ["--samples", "0"]),
	("I_ttf_bad_limits", "ttLib/data/I.ttf", ["--weight-design-limits", "1:4"]),
	("I_ttf_bad_pins", "ttLib/data/I.ttf", ["--weight-pins", "250"]),
	("I_otf", "ttLib/data/I.otf", []),
	("I_otf_sanitize", "ttLib/data/I.otf", ["-s", "--samples", "5"]),
	("Amstelvar_avar2", "ttLib/tables/data/Amstelvar-avar2.subset.ttf", []),
	("Amstelvar_avar2_sanitize", "ttLib/tables/data/Amstelvar-avar2.subset.ttf", ["-s"]),
	("NotoSans_cubic", "ttLib/tables/data/NotoSans-VF-cubic.subset.ttf", []),
	("NotoSans_cubic_sanitize", "ttLib/tables/data/NotoSans-VF-cubic.subset.ttf", ["-s"]),
	("varc_6868", "ttLib/data/varc-6868.ttf", []),
	("varc_ac00_ac01", "ttLib/data/varc-ac00-ac01.ttf", ["-s"]),
	("TestGVAR", "subset/data/TestGVAR.ttx", []),
	("TestHVVAR", "subset/data/TestHVVAR.ttx", ["-s"]),
	("PartialInstancerTest", "varLib/instancer/data/PartialInstancerTest-VF.ttx", ["-s"]),
	("PartialInstancerTest2", "varLib/instancer/data/PartialInstancerTest2-VF.ttx", []),
	("CFF2Instancer_1", "varLib/instancer/data/CFF2Instancer-VF-1.ttx", []),
	("CFF2Instancer_2", "varLib/instancer/data/CFF2Instancer-VF-2.ttx", []),
	("MutatorSans", "varLib/data/MutatorSans_All_Variable.ttx", []),
	("TestCFF2VF", "varLib/data/master_ttx_varfont_otf/TestCFF2VF.ttx", ["-s"]),
	("SparseMasters", "varLib/data/master_ttx_varfont_ttf/SparseMasters-VF.ttx", []),
	(
		"interpolatable_cff2",
		"varLib/data/variable_ttx_interpolatable_cff2/interpolatable-test.ttx",
		["-s"],
	),
	("static_font", "ttLib/data/TestTTF-Regular.ttx", []),
]


def font_file(source, tmp):
	"""The font as a binary file: TTX sources are compiled."""
	path = TESTS / source
	if path.suffix != ".ttx":
		return str(path)
	font = TTFont()
	font.importXML(path)
	out = os.path.join(tmp, path.stem + ".bin")
	font.save(out)
	return out


def run_case(source, args, tmp):
	font = font_file(source, tmp)
	output = os.path.join(tmp, "out.ttf")
	if os.path.exists(output):
		os.remove(output)
	stdout = io.StringIO()
	try:
		with contextlib.redirect_stdout(stdout):
			code = plan.main([font, "-o", output] + args)
	except SystemExit as e:
		return stdout.getvalue() + f"== exit {e.code}\n"
	except Exception as e:
		return f"== error\n{type(e).__name__}: {e}\n"
	text = stdout.getvalue()
	if code:
		return text + f"== exit {code}\n"
	reader = TTFont(output, lazy=True).reader
	avar = reader["avar"].hex() if "avar" in reader else "none"
	return text + f"== avar\n{avar}\n"


def main():
	# Keep the logs quiet: `main` configures the logger on every call.
	fontTools.configLogger = lambda **kwargs: None
	OUT.mkdir(parents=True, exist_ok=True)
	with tempfile.TemporaryDirectory() as tmp:
		for name, source, args in CASES:
			result = run_case(source, args, tmp)
			(OUT / f"{name}.txt").write_text(result, encoding="utf-8", newline="\n")
			print(name)


if __name__ == "__main__":
	main()
