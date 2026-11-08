"""Builds a font with IFT/IFTX tables to probe fontTools' behavior."""

import io
import sys

from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.tables import otTables as ot
from fontTools.ttLib.tables.otConverters import MappingEntryFormat as F


def make_patch_map(entries, entry_data=None, flags=0, url=b"https://x/{id}"):
	pm = ot.PatchMap()
	pm.Format = 2
	pm.Reserved = 0
	pm.Flags = flags
	pm.CompatibilityId = [1, 2, 3, 4]
	pm.DefaultPatchFormat = 1
	pm.NumEntries = len(entries)
	me = ot.MappingEntries()
	me.entries = entries
	pm.MappingEntries = me
	if entry_data is not None:
		d = ot.EntryIdStringData()
		d.data = entry_data
		pm.EntryIdStringData = d
	else:
		pm.EntryIdStringData = None
	pm.UrlTemplateLength = len(url)
	pm.UrlTemplate = list(url)
	if flags & 1:
		pm.CffCharStringsOffset = 1234
	if flags & 2:
		pm.Cff2CharStringsOffset = 5678
	return pm


def main(base, out):
	font = TTFont(base)
	entries = [
		{"formatFlags": F(0x30 | F.HAS_SUBSET_DEF | F.HAS_PATCH_FORMAT), "featureTags": ["liga", "smcp"],
		 "designSpaceSegments": [{"tag": "wght", "start": 100.0, "end": 900.5}],
		 "patchFormat": 2, "codePointsBias": 0x10000, "codePoints": [0x10041, 0x10042, 0x10100]},
		{"formatFlags": F(F.HAS_ENTRY_ID | F.HAS_CHILD_ENTRIES), "entryIds": [5, 3, 10],
		 "childEntryConjunctive": True, "childEntryIndices": [0]},
		{"formatFlags": F(0x10), "codePoints": [65, 66, 67, 200]},
		{"formatFlags": F(0x20), "codePointsBias": 100, "codePoints": list(range(100, 400))},
		{"formatFlags": F(0)},
	]
	t = newTable("IFT ")
	t.table = make_patch_map(entries, entry_data=b"abcdef", flags=1)
	font["IFT "] = t
	t2 = newTable("IFTX")
	t2.table = make_patch_map(entries[2:], flags=2)
	font["IFTX"] = t2
	font.save(out)
	f = TTFont(out)
	buf = io.StringIO()
	f.saveXML(buf, tables=["IFT ", "IFTX"])
	print(buf.getvalue())
	f2 = TTFont()
	try:
		f2.importXML(io.StringIO(buf.getvalue()))
		print("importXML OK")
	except Exception as e:
		print("importXML failed:", type(e).__name__, e)


main(sys.argv[1], sys.argv[2])
