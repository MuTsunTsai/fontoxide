"""Compiles the complete fonts among the TTX files of fontTools' tests.

A TTX file counts as a complete font when it has `head` and `maxp` tables.
Each is compiled with fontTools into `<output dir>/<path with "__" for "/">.ttf`
(or `.otf` for CFF-flavored fonts); files that fontTools cannot compile are
reported and skipped.

Usage: python ttx_fonts.py <fonttools Tests dir> <output dir>
"""

import os
import sys

from fontTools.ttLib import TTFont


def main(tests, out):
	os.makedirs(out, exist_ok=True)
	built = failed = 0
	for root, _, files in os.walk(tests):
		for name in sorted(files):
			if not name.endswith(".ttx"):
				continue
			path = os.path.join(root, name)
			with open(path, encoding="utf-8", errors="replace") as f:
				text = f.read()
			if "<head>" not in text or "<maxp>" not in text or "<ttFont" not in text:
				continue
			rel = os.path.relpath(path, tests).replace(os.sep, "__")[: -len(".ttx")]
			ext = ".otf" if "<CFF " in text or "<CFF>" in text or "<CFF2>" in text else ".ttf"
			target = os.path.join(out, rel + ext)
			try:
				font = TTFont()
				font.importXML(path)
				font.save(target)
				built += 1
			except Exception as e:
				failed += 1
				if os.path.exists(target):
					os.remove(target)
				print(f"{rel}: {type(e).__name__}: {e}")
	print(f"built {built}, failed {failed}")


main(sys.argv[1], sys.argv[2])
