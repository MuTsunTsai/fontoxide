"""Compresses fonts to WOFF 2.0 with each kind of transform and extra data.

For every input font, writes into the output dir:

- `<name>.woff2`: the default transforms (`glyf` and `loca`);
- `<name>.hmtx.woff2`: with the `hmtx` transform too;
- `<name>.none.woff2`: without transforms;
- `<name>.meta.woff2`: with extended metadata and private data.

Fonts that fontTools cannot compress are reported and skipped.

Usage: python woff2_fonts.py <output dir> <font>...
"""

import os
import sys

from fontTools.ttLib import TTFont
from fontTools.ttLib.woff2 import WOFF2FlavorData

METADATA = b"""<?xml version="1.0" encoding="UTF-8"?>
<metadata version="1.0">
	<uniqueid id="org.example.fontoxide.corpus"/>
	<description><text lang="en">A font of the WOFF2 corpus.</text></description>
</metadata>
"""

PRIVATE = b"private data \x00\x01\x02"

VARIANTS = {
	"": dict(transforms=None),
	".hmtx": dict(transforms={"glyf", "loca", "hmtx"}),
	".none": dict(transforms=()),
	".meta": dict(transforms=None, meta=True),
}


def main(out, paths):
	os.makedirs(out, exist_ok=True)
	built = failed = 0
	for path in paths:
		base = os.path.splitext(os.path.basename(path))[0]
		for suffix, options in VARIANTS.items():
			target = os.path.join(out, base + suffix + ".woff2")
			try:
				font = TTFont(path, recalcBBoxes=False, recalcTimestamp=False)
				font.flavor = "woff2"
				font.flavorData = WOFF2FlavorData(transformedTables=options["transforms"])
				if options.get("meta"):
					font.flavorData.metaData = METADATA
					font.flavorData.privData = PRIVATE
				font.save(target, reorderTables=False)
				built += 1
			except Exception as e:
				failed += 1
				if os.path.exists(target):
					os.remove(target)
				print(f"{base}{suffix}: {type(e).__name__}: {e}")
	print(f"built {built}, failed {failed}")


main(sys.argv[1], sys.argv[2:])
