"""Exports CPython's codec tables for `fontoxide-encodings`
(`crates/fontoxide-encodings/data/codecs`).

Usage: uv run --project tools/reference python tools/reference/codecs_export.py [--text DIR]

Single-byte codecs are written as text (`<name>.txt`: a comment line, then the
code point of each byte, `-` where the byte does not decode; they are small). Multi-byte codecs
are written in a compact binary format (`<name>.bin`, big-endian), read by
`fontoxide_encodings::codecs`:

    u16 count, count * (u8 byte, u16 code point)     bytes that decode alone
    u16 count, count * segment                       two-byte sequences
        segment: u16 first sequence, u16 length, length * u16 code point,
        for consecutive sequences; 0xFFFF where a sequence does not decode
    u16 count, count * override                      encodings that differ from
        the first sequence that decodes to the character
        override: u32 code point, u8 length, length bytes (0: does not encode)
    u8 make-up flag, then if 1: 19 + 21 + 28 bytes   the second bytes of the
        EUC-KR make-up jamo (initial, medial, final), for the Hangul syllables
        outside KS X 1001, which encode as A4 D4 A4 i A4 m A4 f

With `--text DIR`, the multi-byte codecs are also written in the previous text
format to DIR, for comparing with an older export.
"""

import struct
import sys
from pathlib import Path

SINGLE_BYTE = [
	"mac_roman",
	"mac_iceland",
	"mac_turkish",
	"mac_croatian",
	"mac_latin2",
	"mac_romanian",
	"mac_greek",
	"mac_cyrillic",
]
MULTI_BYTE = ["shift_jis", "gb2312", "big5", "euc_kr", "johab"]
OUT = Path(__file__).resolve().parents[2] / "crates/fontoxide-encodings/data/codecs"


def decode1(codec, data):
	try:
		s = data.decode(codec)
	except UnicodeDecodeError:
		return None
	return s if len(s) == 1 else None


def tables(codec):
	single = {}
	for b in range(256):
		c = decode1(codec, bytes([b]))
		if c is not None:
			single[b] = ord(c)
	double = {}
	for lead in range(256):
		if lead in single:
			continue
		for trail in range(256):
			c = decode1(codec, bytes([lead, trail]))
			if c is not None:
				double[(lead << 8) | trail] = ord(c)
	# What the decoding tables imply: the first sequence for a character.
	implied = {}
	for b, cp in single.items():
		implied.setdefault(cp, bytes([b]))
	for seq in sorted(double):
		implied.setdefault(double[seq], seq.to_bytes(2, "big"))
	overrides = {}
	for cp in range(0x110000):
		if 0xD800 <= cp <= 0xDFFF:
			continue
		try:
			enc = chr(cp).encode(codec)
		except UnicodeEncodeError:
			enc = None
		want = implied.get(cp)
		if enc != want:
			overrides[cp] = enc
	return single, double, overrides


def makeup(double, overrides):
	"""Splits off the EUC-KR make-up sequences, as jamo tables."""
	long = {cp: b for cp, b in overrides.items() if b is not None and len(b) == 8}
	if not long:
		return None
	jamo = [{}, {}, {}]
	for cp, b in long.items():
		assert b[0] == 0xA4 and b[1] == 0xD4 and b[2] == 0xA4 and b[4] == 0xA4 and b[6] == 0xA4
		s = cp - 0xAC00
		for k, part in enumerate((s // 588, (s % 588) // 28, s % 28)):
			assert jamo[k].setdefault(part, b[3 + 2 * k]) == b[3 + 2 * k]
	sizes = (19, 21, 28)
	assert all(sorted(jamo[k]) == list(range(sizes[k])) for k in range(3))
	tables = [bytes(jamo[k][i] for i in range(sizes[k])) for k in range(3)]
	# The make-up sequences are exactly the syllables that don't decode from two
	# bytes, which is how `fontoxide_encodings::codecs` rebuilds them.
	decodable = set(double.values())
	assert set(long) == {cp for cp in range(0xAC00, 0xD7A4) if cp not in decodable}
	for cp, b in long.items():
		s = cp - 0xAC00
		parts = (s // 588, (s % 588) // 28, s % 28)
		assert b == bytes([0xA4, 0xD4] + sum(([0xA4, tables[k][p]] for k, p in enumerate(parts)), []))
	for k in list(long):
		del overrides[k]
	return tables


def segments(double):
	segs = []
	for seq in sorted(double):
		if segs and segs[-1][0] + len(segs[-1][1]) == seq:
			segs[-1][1].append(double[seq])
		else:
			segs.append((seq, [double[seq]]))
	return segs


def binary(single, double, overrides, jamo):
	out = bytearray()
	out += struct.pack(">H", len(single))
	for b in sorted(single):
		assert single[b] < 0xFFFF
		out += struct.pack(">BH", b, single[b])
	segs = segments(double)
	out += struct.pack(">H", len(segs))
	for first, cps in segs:
		assert all(cp < 0xFFFF for cp in cps)
		out += struct.pack(">HH", first, len(cps))
		out += struct.pack(f">{len(cps)}H", *cps)
	out += struct.pack(">H", len(overrides))
	for cp in sorted(overrides):
		enc = overrides[cp] or b""
		out += struct.pack(">IB", cp, len(enc)) + enc
	if jamo is None:
		out += b"\0"
	else:
		out += b"\1" + b"".join(jamo)
	return bytes(out)


def text(codec, single, double, overrides):
	return "\n".join(
		[
			f"# Python {sys.version.split()[0]} codec {codec}",
			"single " + " ".join(f"{b:02X}:{cp:04X}" for b, cp in sorted(single.items())),
			"double " + " ".join(f"{s:04X}:{cp:04X}" for s, cp in sorted(double.items())),
			"encode "
			+ " ".join(
				f"{cp:04X}:{enc.hex().upper() if enc else '-'}" for cp, enc in sorted(overrides.items())
			),
		]
	) + "\n"


def main():
	text_dir = None
	if sys.argv[1:2] == ["--text"]:
		text_dir = Path(sys.argv[2])
		text_dir.mkdir(parents=True, exist_ok=True)
	for codec in SINGLE_BYTE:
		cps = [decode1(codec, bytes([b])) for b in range(256)]
		line = " ".join("-" if c is None else f"{ord(c):04X}" for c in cps)
		(OUT / f"{codec}.txt").write_bytes(
			f"# Python {sys.version.split()[0]} codec {codec}: byte -> code point\n{line}\n".encode()
		)
	for codec in MULTI_BYTE:
		single, double, overrides = tables(codec)
		if text_dir:
			(text_dir / f"{codec}.txt").write_bytes(text(codec, single, double, dict(overrides)).encode())
		jamo = makeup(double, overrides)
		(OUT / f"{codec}.bin").write_bytes(binary(single, double, overrides, jamo))
		print(codec, len(single), len(double), len(overrides), "make-up" if jamo else "")


if __name__ == "__main__":
	main()
