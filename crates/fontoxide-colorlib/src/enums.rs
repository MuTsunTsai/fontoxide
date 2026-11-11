//! The `IntEnum`s of `fontTools.ttLib.tables.otTables` that colorLib inputs use:
//! `PaintFormat`, `ExtendMode` and `CompositeMode`.

use crate::Value;

macro_rules! int_enum {
	($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident = $value:literal => $py:literal,)* }) => {
		$(#[$doc])*
		#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
		#[repr(u8)]
		pub enum $name {
			$($(#[$vdoc])* $variant = $value,)*
		}

		impl $name {
			/// All members, in definition order.
			pub const ALL: &'static [$name] = &[$($name::$variant,)*];

			/// The member with a value (`EnumClass(value)`).
			pub fn from_value(value: i64) -> Option<Self> {
				match value {
					$($value => Some($name::$variant),)*
					_ => None,
				}
			}

			/// The Python name of the member (`member.name`).
			pub fn py_name(self) -> &'static str {
				match self {
					$($name::$variant => $py,)*
				}
			}

			/// The member with a Python name (`getattr(EnumClass, name)`).
			pub fn from_py_name(name: &str) -> Option<Self> {
				match name {
					$($py => Some($name::$variant),)*
					_ => None,
				}
			}
		}

		impl From<$name> for Value {
			fn from(v: $name) -> Self {
				Value::Int(v as i64)
			}
		}

		impl From<$name> for i64 {
			fn from(v: $name) -> Self {
				v as i64
			}
		}
	};
}

int_enum! {
	/// `otTables.PaintFormat`.
	PaintFormat {
		/// `PaintColrLayers`.
		PaintColrLayers = 1 => "PaintColrLayers",
		/// `PaintSolid`.
		PaintSolid = 2 => "PaintSolid",
		/// `PaintVarSolid`.
		PaintVarSolid = 3 => "PaintVarSolid",
		/// `PaintLinearGradient`.
		PaintLinearGradient = 4 => "PaintLinearGradient",
		/// `PaintVarLinearGradient`.
		PaintVarLinearGradient = 5 => "PaintVarLinearGradient",
		/// `PaintRadialGradient`.
		PaintRadialGradient = 6 => "PaintRadialGradient",
		/// `PaintVarRadialGradient`.
		PaintVarRadialGradient = 7 => "PaintVarRadialGradient",
		/// `PaintSweepGradient`.
		PaintSweepGradient = 8 => "PaintSweepGradient",
		/// `PaintVarSweepGradient`.
		PaintVarSweepGradient = 9 => "PaintVarSweepGradient",
		/// `PaintGlyph`.
		PaintGlyph = 10 => "PaintGlyph",
		/// `PaintColrGlyph`.
		PaintColrGlyph = 11 => "PaintColrGlyph",
		/// `PaintTransform`.
		PaintTransform = 12 => "PaintTransform",
		/// `PaintVarTransform`.
		PaintVarTransform = 13 => "PaintVarTransform",
		/// `PaintTranslate`.
		PaintTranslate = 14 => "PaintTranslate",
		/// `PaintVarTranslate`.
		PaintVarTranslate = 15 => "PaintVarTranslate",
		/// `PaintScale`.
		PaintScale = 16 => "PaintScale",
		/// `PaintVarScale`.
		PaintVarScale = 17 => "PaintVarScale",
		/// `PaintScaleAroundCenter`.
		PaintScaleAroundCenter = 18 => "PaintScaleAroundCenter",
		/// `PaintVarScaleAroundCenter`.
		PaintVarScaleAroundCenter = 19 => "PaintVarScaleAroundCenter",
		/// `PaintScaleUniform`.
		PaintScaleUniform = 20 => "PaintScaleUniform",
		/// `PaintVarScaleUniform`.
		PaintVarScaleUniform = 21 => "PaintVarScaleUniform",
		/// `PaintScaleUniformAroundCenter`.
		PaintScaleUniformAroundCenter = 22 => "PaintScaleUniformAroundCenter",
		/// `PaintVarScaleUniformAroundCenter`.
		PaintVarScaleUniformAroundCenter = 23 => "PaintVarScaleUniformAroundCenter",
		/// `PaintRotate`.
		PaintRotate = 24 => "PaintRotate",
		/// `PaintVarRotate`.
		PaintVarRotate = 25 => "PaintVarRotate",
		/// `PaintRotateAroundCenter`.
		PaintRotateAroundCenter = 26 => "PaintRotateAroundCenter",
		/// `PaintVarRotateAroundCenter`.
		PaintVarRotateAroundCenter = 27 => "PaintVarRotateAroundCenter",
		/// `PaintSkew`.
		PaintSkew = 28 => "PaintSkew",
		/// `PaintVarSkew`.
		PaintVarSkew = 29 => "PaintVarSkew",
		/// `PaintSkewAroundCenter`.
		PaintSkewAroundCenter = 30 => "PaintSkewAroundCenter",
		/// `PaintVarSkewAroundCenter`.
		PaintVarSkewAroundCenter = 31 => "PaintVarSkewAroundCenter",
		/// `PaintComposite`.
		PaintComposite = 32 => "PaintComposite",
		/// `PaintGlyph2`.
		PaintGlyph2 = 33 => "PaintGlyph2",
	}
}

impl PaintFormat {
	/// `is_variable`: whether the name starts with `PaintVar`.
	pub fn is_variable(self) -> bool {
		self.py_name().starts_with("PaintVar")
	}
}

int_enum! {
	/// `otTables.ExtendMode`.
	ExtendMode {
		/// `PAD`.
		Pad = 0 => "PAD",
		/// `REPEAT`.
		Repeat = 1 => "REPEAT",
		/// `REFLECT`.
		Reflect = 2 => "REFLECT",
	}
}

int_enum! {
	/// `otTables.CompositeMode`.
	CompositeMode {
		/// `CLEAR`.
		Clear = 0 => "CLEAR",
		/// `SRC`.
		Src = 1 => "SRC",
		/// `DEST`.
		Dest = 2 => "DEST",
		/// `SRC_OVER`.
		SrcOver = 3 => "SRC_OVER",
		/// `DEST_OVER`.
		DestOver = 4 => "DEST_OVER",
		/// `SRC_IN`.
		SrcIn = 5 => "SRC_IN",
		/// `DEST_IN`.
		DestIn = 6 => "DEST_IN",
		/// `SRC_OUT`.
		SrcOut = 7 => "SRC_OUT",
		/// `DEST_OUT`.
		DestOut = 8 => "DEST_OUT",
		/// `SRC_ATOP`.
		SrcAtop = 9 => "SRC_ATOP",
		/// `DEST_ATOP`.
		DestAtop = 10 => "DEST_ATOP",
		/// `XOR`.
		Xor = 11 => "XOR",
		/// `PLUS`.
		Plus = 12 => "PLUS",
		/// `SCREEN`.
		Screen = 13 => "SCREEN",
		/// `OVERLAY`.
		Overlay = 14 => "OVERLAY",
		/// `DARKEN`.
		Darken = 15 => "DARKEN",
		/// `LIGHTEN`.
		Lighten = 16 => "LIGHTEN",
		/// `COLOR_DODGE`.
		ColorDodge = 17 => "COLOR_DODGE",
		/// `COLOR_BURN`.
		ColorBurn = 18 => "COLOR_BURN",
		/// `HARD_LIGHT`.
		HardLight = 19 => "HARD_LIGHT",
		/// `SOFT_LIGHT`.
		SoftLight = 20 => "SOFT_LIGHT",
		/// `DIFFERENCE`.
		Difference = 21 => "DIFFERENCE",
		/// `EXCLUSION`.
		Exclusion = 22 => "EXCLUSION",
		/// `MULTIPLY`.
		Multiply = 23 => "MULTIPLY",
		/// `HSL_HUE`.
		HslHue = 24 => "HSL_HUE",
		/// `HSL_SATURATION`.
		HslSaturation = 25 => "HSL_SATURATION",
		/// `HSL_COLOR`.
		HslColor = 26 => "HSL_COLOR",
		/// `HSL_LUMINOSITY`.
		HslLuminosity = 27 => "HSL_LUMINOSITY",
	}
}
