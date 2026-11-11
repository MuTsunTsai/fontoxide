"""Generates the fixtures of crates/fontoxide-varlib/tests/data/interpolatable.

fontTools' Tests/varLib/interpolatable_test.py only checks a few lines of the
reports of `fontTools.varLib.interpolatable.main`, and its test fonts have
hardly any problems. This script saves, for each font set the tests use and
for a few more:

- <case>.txt: the output of `main` with several options (text and JSON
  reports, `--show-all`) and the problems it returns.

fontoxide matches fontTools with all its extras, so SciPy (a dependency of the
reference environment) is used; the fallbacks fontTools takes without it are
not ported, and have no fixtures.

It also builds synthetic masters with every kind of problem (contour order,
starting points, reversed contours, kinks, underweight contours, incompatible
nodes, missing glyphs, components, open contours) as synthetic/*.ttx and
synthetic/*.ufo, and saves the results of `scipy.optimize.linear_sum_assignment`
and `scipy.sparse.csgraph.minimum_spanning_tree` on random matrices with ties
(lsap.txt, mst.txt).

Run with:

    uv run --project tools/reference python tools/reference/interpolatable_fixtures.py

The Rust tests compile the TTX files themselves, and build the layered
designspaces of the upstream tests in a temporary directory.
"""

import io
import json
import os
import pathlib
import random
import shutil
import sys
import tempfile
from contextlib import redirect_stdout

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from scipy.optimize import linear_sum_assignment
from scipy.sparse.csgraph import minimum_spanning_tree

from fontTools.designspaceLib import DesignSpaceDocument
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.beyond64k import upper_tables
from fontTools.ufoLib import UFOWriter
from fontTools.varLib.interpolatable import main

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "varLib" / "data"
TTLIB_DATA = ROOT / "upstream" / "fonttools" / "Tests" / "ttLib" / "data"
OUT = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "interpolatable"
SYNTH = OUT / "synthetic"

# The option sets each case is run with.
VARIANTS = [[], ["--json"], ["--show-all"], ["--show-all", "--json"], ["--ignore-missing"]]


def synthetic_variants(small):
	"""The option sets of the synthetic masters: `small` are the glyphs with at
	most six contours."""
	return [
		[],
		["--json"],
		["--quiet", "--tolerance", "0.99", "--kinkiness", "0.2"],
		["--show-all", "--glyphs", " ".join(small)],
	]


def compile_font(path, suffix, temp_dir):
	"""`compile_font` of the test."""
	save = os.path.join(temp_dir, os.path.basename(path).replace(".ttx", suffix))
	font = TTFont(recalcBBoxes=False, recalcTimestamp=False)
	font.importXML(str(path))
	font.save(save, reorderTables=None)
	return font, save


def file_list(folder, suffix, prefix=""):
	return sorted(
		os.path.abspath(os.path.join(folder, p))
		for p in os.listdir(folder)
		if p.startswith(prefix) and p.endswith(suffix)
	)


def run(args, inputs):
	"""The output of `main(args + inputs)`, and what it returns or raises."""
	out = io.StringIO()
	# The options, separated by tabs.
	header = "\t".join(args)
	try:
		with redirect_stdout(out):
			problems = main(args + inputs)
		result = "--- return\n" + ("None" if problems is None else json.dumps(problems))
	except Exception as e:
		result = "--- error\n" + str(e.args[0])
	return "=== %s\n%s%s\n" % (header, out.getvalue(), result)


def save_case(name, inputs, variants=VARIANTS):
	text = "".join(run(args, inputs) for args in variants)
	(OUT / (name + ".txt")).write_text(text, encoding="utf-8", newline="\n")


# ---------------------------------------------------------------------------
# Synthetic masters


class Contour:
	"""A closed contour: a start point, then segments ("line", pt) or
	("curve", [off-curve points], pt)."""

	def __init__(self, start, segments):
		self.start = start
		self.segments = segments

	def points(self):
		return [self.start] + [s[-1] for s in self.segments]

	def map(self, f):
		return Contour(
			f(self.start),
			[(s[0], [f(p) for p in s[1]], f(s[2])) if s[0] == "curve" else (s[0], f(s[1])) for s in self.segments],
		)

	def rotated(self, k):
		"""Starting at on-curve point k (the end of segment k - 1)."""
		k %= len(self.segments)
		if k == 0:
			return self
		start = self.segments[k - 1][-1]
		return Contour(start, self.segments[k:] + self.segments[:k])

	def reversed(self):
		ons = [self.start] + [s[-1] for s in self.segments]
		segments = []
		for i in range(len(self.segments) - 1, -1, -1):
			s = self.segments[i]
			end = ons[i]
			if s[0] == "line":
				segments.append(("line", end))
			else:
				segments.append(("curve", list(reversed(s[1])), end))
		return Contour(self.segments[-1][-1], segments)

	def draw(self, pen, quadratic, close=True):
		pen.moveTo(self.start)
		for s in self.segments:
			if s[0] == "line":
				pen.lineTo(s[1])
			elif quadratic:
				pen.qCurveTo(*s[1][:1], s[2])
			else:
				pen.curveTo(*s[1], s[2])
		if close:
			pen.closePath()
		else:
			pen.endPath()


def ellipse(rng, cx, cy, rx, ry, n, quadratic, curve_ratio=0.7):
	"""A contour around an ellipse, counter-clockwise for TrueType."""
	import math

	angles = sorted(rng.uniform(0, 2 * math.pi) for _ in range(n))
	pts = [(cx + rx * math.cos(a), cy + ry * math.sin(a)) for a in angles]
	segments = []
	for i in range(n):
		p0 = pts[i]
		p1 = pts[(i + 1) % n]
		if rng.random() < curve_ratio:
			a0 = angles[i]
			a1 = angles[(i + 1) % n] + (2 * math.pi if i + 1 == n else 0)
			if quadratic:
				am = (a0 + a1) / 2
				k = 1 / math.cos((a1 - a0) / 2) if abs(a1 - a0) < 2.5 else 1.5
				segments.append(("curve", [(cx + rx * k * math.cos(am), cy + ry * k * math.sin(am))], p1))
			else:
				h = 4 / 3 * math.tan((a1 - a0) / 4)
				c1 = (p0[0] - h * rx * math.sin(a0), p0[1] + h * ry * math.cos(a0))
				c2 = (p1[0] + h * rx * math.sin(a1), p1[1] - h * ry * math.cos(a1))
				segments.append(("curve", [c1, c2], p1))
		else:
			segments.append(("line", p1))
	return Contour(pts[0], segments)


def rounded(c):
	return c.map(lambda p: (round(p[0]), round(p[1])))


def kink_contour(target, quadratic):
	"""A contour with a smooth point whose handles change their lengths and
	turn by 30 degrees: a kink halfway."""
	import math

	if not target:
		u, a, b = (1, 0), 100, 20
	else:
		u, a, b = (math.cos(math.pi / 6), math.sin(math.pi / 6)), 20, 100
	p = (300, 300)
	c_prev = (p[0] - a * u[0], p[1] - a * u[1])
	c_next = (p[0] + b * u[0], p[1] + b * u[1])
	if quadratic:
		segs = [("line", (500, 100)), ("curve", [c_prev], p), ("curve", [c_next], (100, 500))]
	else:
		segs = [
			("line", (500, 100)),
			("curve", [(450, 150), c_prev], p),
			("curve", [c_next, (300, 600)], (100, 500)),
		]
	return Contour((100, 100), segs)

def synthetic_glyphs(rng, quadratic, masters=3):
	"""The glyphs of each master: name -> list of contours or ("component",
	name, (dx, dy)) entries."""
	import math

	out = [dict() for _ in range(masters)]
	names = []

	def add(name, per_master):
		names.append(name)
		for m in range(masters):
			if per_master[m] is not None:
				out[m][name] = per_master[m]

	def base_contours(n, quad=quadratic):
		cs = []
		for i in range(n):
			cx = 150 + (i % 3) * 250 + rng.uniform(-20, 20)
			cy = 150 + (i // 3) * 250 + rng.uniform(-20, 20)
			r = rng.uniform(40, 110)
			cs.append(ellipse(rng, cx, cy, r, r * rng.uniform(0.6, 1.4), rng.randint(3, 9), quad))
		return cs

	def weight(cs, m):
		"""Bolder masters: each contour scaled around its center."""
		res = []
		for c in cs:
			pts = c.points()
			cx = sum(p[0] for p in pts) / len(pts)
			cy = sum(p[1] for p in pts) / len(pts)
			s = 1 + 0.06 * m
			res.append(c.map(lambda p: (cx + (p[0] - cx) * s, cy + (p[1] - cy) * s)))
		return res

	kinds = [
		"plain",
		"order",
		"order_many",
		"start",
		"start_one",
		"reverse",
		"rotate90",
		"kink",
		"nodes_type",
		"nodes_count",
		"paths",
		"missing",
		"component",
		"component_order",
		"mixed",
	]
	for kind in kinds:
		for variant in range(3):
			name = "%s.%d" % (kind, variant)
			n = rng.randint(2, 9) if kind != "order_many" else rng.randint(7, 12)
			if kind in ("start_one",):
				n = 1
			base = base_contours(n)
			per = [weight(base, m) for m in range(masters)]
			target = rng.randrange(1, masters)
			if kind in ("order", "order_many", "mixed"):
				order = list(range(n))
				while order == list(range(n)) and n > 1:
					rng.shuffle(order)
				per[target] = [per[target][i] for i in order]
			if kind in ("start", "start_one", "mixed"):
				i = rng.randrange(len(per[target]))
				c = per[target][i]
				per[target][i] = c.rotated(rng.randint(1, len(c.segments)))
			if kind == "reverse":
				i = rng.randrange(len(per[target]))
				per[target][i] = per[target][i].reversed()
			if kind == "rotate90":
				i = rng.randrange(len(per[target]))
				c = per[target][i]
				pts = c.points()
				cx = sum(p[0] for p in pts) / len(pts)
				cy = sum(p[1] for p in pts) / len(pts)
				per[target][i] = c.map(lambda p: (cx - (p[1] - cy), cy + (p[0] - cx)))
			if kind == "kink":
				for m in range(masters):
					per[m].append(kink_contour(m == target, quadratic))
			if kind == "nodes_type":
				i = rng.randrange(len(per[target]))
				c = per[target][i]
				segs = list(c.segments)
				j = rng.randrange(len(segs))
				s = segs[j]
				if s[0] == "line":
					prev = c.points()[j]
					mid = ((prev[0] + s[1][0]) / 2, (prev[1] + s[1][1]) / 2)
					segs[j] = ("curve", [mid] if quadratic else [mid, mid], s[1])
				else:
					segs[j] = ("line", s[2])
				per[target][i] = Contour(c.start, segs)
			if kind == "nodes_count":
				i = rng.randrange(len(per[target]))
				c = per[target][i]
				if len(c.segments) > 2:
					per[target][i] = Contour(c.start, c.segments[:-2] + [("line", c.segments[-1][-1])])
			if kind == "paths":
				per[target] = per[target][:-1]
			if kind == "missing":
				per[target] = None
			per = [None if p is None else [rounded(c) for c in p] for p in per]
			add(name, per)

	# Components of the plain glyphs.
	for variant in range(3):
		name = "composite.%d" % variant
		comps = [("component", "plain.%d" % variant, (0, 0)), ("component", "start.%d" % variant, (50, 0))]
		per = [list(comps) for _ in range(masters)]
		if variant == 1:
			per[1] = list(reversed(per[1]))
		if variant == 2:
			per[2] = per[2] + [("component", "reverse.0", (0, 300))]
		add(name, per)
	small = [n for n in names if all(len(m.get(n, ())) <= 6 for m in out)]
	return names, out, small


def draw_glyph(pen, items, quadratic, glyphs=None):
	"""Draws the items of a glyph; with `glyphs`, the components are drawn
	decomposed (CFF has no components)."""
	for item in items:
		if isinstance(item, tuple) and item[0] == "component":
			_, base, (dx, dy) = item
			if glyphs is None:
				pen.addComponent(base, (1, 0, 0, 1, dx, dy))
			else:
				from fontTools.pens.transformPen import TransformPen

				draw_glyph(TransformPen(pen, (1, 0, 0, 1, dx, dy)), glyphs[base], quadratic, glyphs)
		else:
			item.draw(pen, quadratic)


def build_binary_masters(path, rng, quadratic, style_prefix):
	names, masters, small = synthetic_glyphs(rng, quadratic)
	paths = []
	for m, glyphs in enumerate(masters):
		order = [".notdef"] + [n for n in names if n in glyphs]
		fb = FontBuilder(1000, isTTF=quadratic)
		fb.setupGlyphOrder(order)
		fb.setupCharacterMap({})
		if quadratic:
			ttglyphs = {".notdef": TTGlyphPen(None).glyph()}
			for n in order[1:]:
				pen = TTGlyphPen(ttglyphs)
				draw_glyph(pen, glyphs[n], True)
				ttglyphs[n] = pen.glyph()
			fb.setupGlyf(ttglyphs)
		else:
			charstrings = {}
			for n in order:
				pen = T2CharStringPen(500, None)
				if n != ".notdef":
					draw_glyph(pen, glyphs[n], False, glyphs)
				charstrings[n] = pen.getCharString()
			fb.setupCFF("Synthetic-%s%d" % (style_prefix, m), {"FullName": "Synthetic"}, charstrings, {})
		fb.setupHorizontalMetrics({n: (500, 0) for n in order})
		fb.setupHorizontalHeader(ascent=800, descent=-200)
		fb.setupNameTable({"familyName": "Synthetic", "styleName": "%s%d" % (style_prefix, m)})
		fb.setupOS2()
		fb.setupPost()
		ttx = path / ("Synthetic%s-Master%d.ttx" % (style_prefix, m))
		fb.font.saveXML(str(ttx))
		paths.append(ttx)
	return paths, small


def build_ufo_masters(path, rng):
	"""Cubic UFO masters (SyntheticUFO), and masters with open contours
	(SyntheticOpen)."""
	names, masters, small = synthetic_glyphs(rng, False)
	paths = write_ufos(path, "SyntheticUFO", names, masters)
	names = []
	masters = [dict() for _ in range(3)]
	for i in range(3):
		name = "open.%d" % i
		names.append(name)
		for m, glyphs in enumerate(masters):
			cs = [rounded(ellipse(rng, 300 + 200 * k, 300, 100, 80, 5, False)) for k in range(i + 1)]
			glyphs[name] = [
				("open", Contour(c.start, c.segments[:-1])) if (k == 0 and m == 1) else ("closed", c)
				for k, c in enumerate(cs)
			]
	return paths, small, write_ufos(path, "SyntheticOpen", names, masters)


def write_ufos(path, family, names, masters):
	paths = []
	for m, glyphs in enumerate(masters):
		ufo = path / ("%s-Master%d.ufo" % (family, m))
		if ufo.exists():
			shutil.rmtree(ufo)
		with UFOWriter(ufo, formatVersion=3) as writer:
			writer.writeInfo(SimpleInfo(1000))
			glyphset = writer.getGlyphSet()
			for n in names:
				if n not in glyphs:
					continue
				items = glyphs[n]

				def drawPoints(pen, items=items):
					from fontTools.pens.pointPen import SegmentToPointPen

					seg = SegmentToPointPen(pen)
					for item in items:
						if isinstance(item, tuple) and item[0] == "component":
							_, base, (dx, dy) = item
							seg.addComponent(base, (1, 0, 0, 1, dx, dy))
						elif isinstance(item, tuple):
							item[1].draw(seg, False, close=item[0] == "closed")
						else:
							item.draw(seg, False)

				glyphset.writeGlyph(n, SimpleGlyph(), drawPoints)
			glyphset.writeContents()
			writer.writeLayerContents()
		paths.append(ufo)
	return paths


class SimpleInfo:
	def __init__(self, upem):
		self.unitsPerEm = upem


class SimpleGlyph:
	width = 500


# ---------------------------------------------------------------------------


def lsap_vectors(rng):
	cases = []
	for _ in range(200):
		n = rng.randint(1, 9)
		kind = rng.random()
		if kind < 0.4:
			m = [[float(rng.randint(0, 3)) for _ in range(n)] for _ in range(n)]
		elif kind < 0.6:
			v = rng.random()
			m = [[v for _ in range(n)] for _ in range(n)]
		else:
			m = [[rng.random() * 1000 for _ in range(n)] for _ in range(n)]
		rows, cols = linear_sum_assignment(m)
		# The rows of the matrix, then the assignment.
		cases.append(" | ".join([" ".join(repr(x) for x in row) for row in m] + [" ".join(str(int(c)) for c in cols)]))
	return "\n".join(cases) + "\n"


def mst_vectors(rng):
	cases = []
	for _ in range(150):
		n = rng.randint(1, 8)
		g = [[0.0] * n for _ in range(n)]
		for i in range(n):
			for j in range(i + 1, n):
				r = rng.random()
				if r < 0.2:
					g[i][j] = 0.0
				elif r < 0.6:
					g[i][j] = float(rng.randint(1, 3))
				else:
					g[i][j] = rng.random()
		tree = minimum_spanning_tree(g)
		rows, cols = tree.nonzero()
		# The rows of the graph, then the edges of the tree.
		edges = " ".join("%d,%d" % (int(r), int(c)) for r, c in zip(rows, cols))
		cases.append(" | ".join([" ".join(repr(x) for x in row) for row in g] + [edges]))
	return "\n".join(cases) + "\n"


def main_():
	OUT.mkdir(parents=True, exist_ok=True)
	SYNTH.mkdir(parents=True, exist_ok=True)
	rng = random.Random(20261111)

	(OUT / "lsap.txt").write_text(lsap_vectors(rng), encoding="utf-8", newline="\n")
	(OUT / "mst.txt").write_text(mst_vectors(rng), encoding="utf-8", newline="\n")

	synth_ttf = build_binary_masters(SYNTH, random.Random(1), True, "TTF")
	synth_otf = build_binary_masters(SYNTH, random.Random(2), False, "OTF")
	synth_ufo, small_ufo, synth_open = build_ufo_masters(SYNTH, random.Random(3))

	with tempfile.TemporaryDirectory() as tmp:
		def compile_all(folder, prefix, suffix):
			for p in file_list(folder, ".ttx", prefix):
				compile_font(p, suffix, tmp)
			return file_list(tmp, suffix, prefix)

		ttx = DATA / "master_ttx_interpolatable_ttf"
		for prefix in ["TestFamily2-", "TestFamily-", "TestFamily3-", "TestFamily4-", "SparseMasters-", "InconsistentUseMyMetrics-"]:
			save_case("ttf_" + prefix.rstrip("-"), compile_all(ttx, prefix, ".ttf"))
		sparse = file_list(tmp, ".ttf", "SparseMasters-")
		save_case("ttf_SparseMasters_medium_first", [sparse[1], sparse[0], sparse[2]], [[], ["--ignore-missing"]])
		save_case("ttf_SparseMasters_medium_last", [sparse[0], sparse[2], sparse[1]], [[], ["--ignore-missing"]])
		save_case("otf_TestFamily2", compile_all(DATA / "master_ttx_interpolatable_otf", "TestFamily2-", ".otf"))
		save_case("cff2", compile_all(DATA / "variable_ttx_interpolatable_cff2", "interpolatable-test", ".otf"))
		for p in file_list(DATA / "master_ttx_varfont_ttf", ".ttx"):
			_, path = compile_font(p, ".ttf", tmp)
			save_case("vf_" + os.path.basename(p)[:-4], [path])

		font, path = compile_font(DATA / "MutatorSans_All_Variable.ttx", ".ttf", tmp)
		upper_tables(font)
		font.save(path, reorderTables=None)
		save_case("beyond64k", [path])

		varc = str(TTLIB_DATA / "varc-ac00-ac01.ttf")
		save_case("varc", [varc])

		synth_dir = os.path.join(tmp, "synth")
		os.mkdir(synth_dir)
		for kind, (paths, small), suffix in (("ttf", synth_ttf, ".ttf"), ("otf", synth_otf, ".otf")):
			compiled = [compile_font(p, suffix, synth_dir)[1] for p in paths]
			save_case("synthetic_" + kind, compiled, synthetic_variants(small))
			save_case("synthetic_%s_reordered" % kind, [compiled[2], compiled[0], compiled[1]], [["--json"]])

	ufo = DATA / "master_ufo"
	for prefix in ["TestFamily2-", "TestFamily-", "TestFamily3-", "TestFamily4-", "SparseMasters-"]:
		save_case("ufo_" + prefix.rstrip("-"), file_list(ufo, ".ufo", prefix))
	sparse = file_list(ufo, ".ufo", "SparseMasters-")
	save_case("ufo_SparseMasters_medium_first", [sparse[1], sparse[0], sparse[2]], [[], ["--ignore-missing"]])
	save_case("ufo_SparseMasters_medium_last", [sparse[0], sparse[2], sparse[1]], [[], ["--ignore-missing"]])
	save_case("synthetic_ufo", [str(p) for p in synth_ufo], synthetic_variants(small_ufo))
	save_case("synthetic_open", [str(p) for p in synth_open], [["--glyphs", g] for g in ("open.0", "open.1", "open.2")])
	save_case("ds_InterpolateLayout", [str(DATA / "InterpolateLayout.designspace")])
	save_case("ds_SparseMasters", [str(DATA / "SparseMasters_ufo.designspace")])

	with tempfile.TemporaryDirectory() as tmp:
		doc = DesignSpaceDocument.fromfile(DATA / "InterpolateLayout.designspace")
		doc.axes[0].map = [(0, 1000), (1000, 0)]
		for source in doc.sources:
			source.location = {"weight": 1000 - source.location["weight"]}
			source.filename = source.path
		path = os.path.join(tmp, "decreasing.designspace")
		with open(path, "wb") as f:
			f.write(doc.tostring())
		save_case("ds_decreasing", [path])


if __name__ == "__main__":
	main_()
