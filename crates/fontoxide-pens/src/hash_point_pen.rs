//! A pen hashing the outline of a glyph (`fontTools.pens.hashPointPen`).

use fontoxide_misc::py::{float_repr, format_g_lower, round_digits};

use crate::{Error, GlyphSet, Point, PointPen, Result, SegmentType, Transform};

/// Builds a string identifying the outline and the components of a glyph,
/// to check whether it changed (`HashPointPen`). Components are drawn from
/// the glyph set, followed by their transformation.
///
/// fontTools formats integers and floats differently (`+1` and `+1.0`); here
/// every integral value is formatted as an integer, as for the glyph width,
/// the points and the components of TrueType glyphs without a scale.
pub struct HashPointPen<'g> {
	glyph_set: Option<&'g dyn GlyphSet>,
	data: Vec<String>,
}

impl std::fmt::Debug for HashPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("HashPointPen")
			.field("data", &self.data)
			.finish_non_exhaustive()
	}
}

/// `str()` of a value, as an integer if it is integral.
fn num(x: f64) -> String {
	if x.fract() == 0.0 && x.abs() < 1e16 {
		format!("{}", x as i64)
	} else {
		float_repr(x)
	}
}

/// `f"{x:+}"`.
fn signed(x: f64) -> String {
	let s = num(x);
	if s.starts_with('-') {
		s
	} else {
		format!("+{s}")
	}
}

impl<'g> HashPointPen<'g> {
	/// `HashPointPen(glyphWidth, glyphSet)`.
	pub fn new(glyph_width: f64, glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		HashPointPen {
			glyph_set,
			data: vec![format!("w{}", num(round_digits(glyph_width, 9)))],
		}
	}

	/// `hash`: the data, or its SHA-512 digest in hexadecimal if it is 128
	/// characters or longer.
	pub fn hash(&self) -> String {
		let data = self.data.concat();
		if data.len() >= 128 {
			sha512_hex(data.as_bytes())
		} else {
			data
		}
	}
}

impl PointPen for HashPointPen<'_> {
	fn begin_path(&mut self, _: Option<&str>) -> Result<()> {
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.data.push("|".into());
		Ok(())
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		_: bool,
		_: Option<&str>,
		_: Option<&str>,
	) -> Result<()> {
		let pt_type = segment_type.map_or('o', |s| s.as_str().as_bytes()[0] as char);
		let y = format_g_lower(pt.1, 6);
		let y = if y.starts_with('-') {
			y
		} else {
			format!("+{y}")
		};
		self.data
			.push(format!("{pt_type}{}{y}", format_g_lower(pt.0, 6)));
		Ok(())
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		_: Option<&str>,
	) -> Result<()> {
		let tr: String = transformation
			.to_array()
			.iter()
			.map(|&t| signed(t))
			.collect();
		self.data.push("[".into());
		let glyph_set = self
			.glyph_set
			.ok_or_else(|| Error::MissingComponent(base_glyph_name.to_owned()))?;
		if !glyph_set.draw_glyph_points(base_glyph_name, self)? {
			return Err(Error::MissingComponent(base_glyph_name.to_owned()));
		}
		self.data.push(format!("({tr})]"));
		Ok(())
	}
}

/// SHA-512 (FIPS 180-4), as a lowercase hexadecimal string.
fn sha512_hex(data: &[u8]) -> String {
	const K: [u64; 80] = [
		0x428a2f98d728ae22,
		0x7137449123ef65cd,
		0xb5c0fbcfec4d3b2f,
		0xe9b5dba58189dbbc,
		0x3956c25bf348b538,
		0x59f111f1b605d019,
		0x923f82a4af194f9b,
		0xab1c5ed5da6d8118,
		0xd807aa98a3030242,
		0x12835b0145706fbe,
		0x243185be4ee4b28c,
		0x550c7dc3d5ffb4e2,
		0x72be5d74f27b896f,
		0x80deb1fe3b1696b1,
		0x9bdc06a725c71235,
		0xc19bf174cf692694,
		0xe49b69c19ef14ad2,
		0xefbe4786384f25e3,
		0x0fc19dc68b8cd5b5,
		0x240ca1cc77ac9c65,
		0x2de92c6f592b0275,
		0x4a7484aa6ea6e483,
		0x5cb0a9dcbd41fbd4,
		0x76f988da831153b5,
		0x983e5152ee66dfab,
		0xa831c66d2db43210,
		0xb00327c898fb213f,
		0xbf597fc7beef0ee4,
		0xc6e00bf33da88fc2,
		0xd5a79147930aa725,
		0x06ca6351e003826f,
		0x142929670a0e6e70,
		0x27b70a8546d22ffc,
		0x2e1b21385c26c926,
		0x4d2c6dfc5ac42aed,
		0x53380d139d95b3df,
		0x650a73548baf63de,
		0x766a0abb3c77b2a8,
		0x81c2c92e47edaee6,
		0x92722c851482353b,
		0xa2bfe8a14cf10364,
		0xa81a664bbc423001,
		0xc24b8b70d0f89791,
		0xc76c51a30654be30,
		0xd192e819d6ef5218,
		0xd69906245565a910,
		0xf40e35855771202a,
		0x106aa07032bbd1b8,
		0x19a4c116b8d2d0c8,
		0x1e376c085141ab53,
		0x2748774cdf8eeb99,
		0x34b0bcb5e19b48a8,
		0x391c0cb3c5c95a63,
		0x4ed8aa4ae3418acb,
		0x5b9cca4f7763e373,
		0x682e6ff3d6b2b8a3,
		0x748f82ee5defb2fc,
		0x78a5636f43172f60,
		0x84c87814a1f0ab72,
		0x8cc702081a6439ec,
		0x90befffa23631e28,
		0xa4506cebde82bde9,
		0xbef9a3f7b2c67915,
		0xc67178f2e372532b,
		0xca273eceea26619c,
		0xd186b8c721c0c207,
		0xeada7dd6cde0eb1e,
		0xf57d4f7fee6ed178,
		0x06f067aa72176fba,
		0x0a637dc5a2c898a6,
		0x113f9804bef90dae,
		0x1b710b35131c471b,
		0x28db77f523047d84,
		0x32caab7b40c72493,
		0x3c9ebe0a15c9bebc,
		0x431d67c49c100d4c,
		0x4cc5d4becb3e42b6,
		0x597f299cfc657e2a,
		0x5fcb6fab3ad6faec,
		0x6c44198c4a475817,
	];
	let mut h: [u64; 8] = [
		0x6a09e667f3bcc908,
		0xbb67ae8584caa73b,
		0x3c6ef372fe94f82b,
		0xa54ff53a5f1d36f1,
		0x510e527fade682d1,
		0x9b05688c2b3e6c1f,
		0x1f83d9abfb41bd6b,
		0x5be0cd19137e2179,
	];
	let mut msg = data.to_vec();
	let bit_len = (data.len() as u128) * 8;
	msg.push(0x80);
	while msg.len() % 128 != 112 {
		msg.push(0);
	}
	msg.extend_from_slice(&bit_len.to_be_bytes());
	for block in msg.as_chunks::<128>().0 {
		let mut w = [0u64; 80];
		for (i, word) in block.as_chunks::<8>().0.iter().enumerate() {
			w[i] = u64::from_be_bytes(*word);
		}
		for i in 16..80 {
			let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
			let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
			w[i] = w[i - 16]
				.wrapping_add(s0)
				.wrapping_add(w[i - 7])
				.wrapping_add(s1);
		}
		let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
		for i in 0..80 {
			let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
			let ch = (e & f) ^ (!e & g);
			let t1 = hh
				.wrapping_add(s1)
				.wrapping_add(ch)
				.wrapping_add(K[i])
				.wrapping_add(w[i]);
			let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
			let maj = (a & b) ^ (a & c) ^ (b & c);
			let t2 = s0.wrapping_add(maj);
			hh = g;
			g = f;
			f = e;
			e = d.wrapping_add(t1);
			d = c;
			c = b;
			b = a;
			a = t1.wrapping_add(t2);
		}
		for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
			*x = x.wrapping_add(y);
		}
	}
	h.iter().map(|x| format!("{x:016x}")).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sha512() {
		assert_eq!(
			sha512_hex(b"abc"),
			"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
			 2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
				.replace(['\t', ' ', '\n'], "")
		);
	}
}
