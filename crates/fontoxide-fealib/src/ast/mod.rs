//! The abstract syntax tree of feature files (`fontTools.feaLib.ast`).
//!
//! Every Python AST class is a Rust type with the same name and its attributes
//! as `snake_case` fields; every node keeps its [`Location`](crate::Location). Statements are the
//! variants of [`Statement`]; the "glyph-containing objects" (`GlyphName`,
//! `GlyphClass`, `GlyphClassName`, `MarkClassName`, `NullGlyph`) are the variants
//! of [`GlyphContainer`].
//!
//! Objects that Python shares by reference are shared here too:
//!
//! - a [`GlyphClassDefinition`] is an `Rc`, shared by the [`GlyphClassName`]s that
//!   refer to it;
//! - a [`LookupBlock`] is an `Rc`, shared by lookup references and the lookups of
//!   chaining rules;
//! - a [`MarkClass`] is an `Rc<RefCell<_>>` ([`MarkClassRef`]), shared by
//!   [`FeatureFile::mark_classes`], [`MarkClassName`]s and the marks of
//!   mark attachment rules. Mark classes grow as `markClass` statements are
//!   parsed, so a reference sees all definitions of the file, like in Python;
//! - a [`MarkClassDefinition`] is an `Rc`, owned by its statement and by its
//!   mark class.
//!
//! `asFea()` is `as_fea()`. It returns a `Result` where Python can raise.

mod expr;
mod stmt;

pub use expr::*;
pub use stmt::*;

/// The indentation step of `asFea()` (`SHIFT`).
pub const SHIFT: &str = "    ";

/// The keywords that are escaped with a backslash when used as glyph names
/// (`fea_keywords`).
pub const FEA_KEYWORDS: &[&str] = &[
	"anchor",
	"anchordef",
	"anon",
	"anonymous",
	"by",
	"contour",
	"cursive",
	"device",
	"enum",
	"enumerate",
	"excludedflt",
	"exclude_dflt",
	"feature",
	"from",
	"ignore",
	"ignorebaseglyphs",
	"ignoreligatures",
	"ignoremarks",
	"include",
	"includedflt",
	"include_dflt",
	"language",
	"languagesystem",
	"lookup",
	"lookupflag",
	"mark",
	"markattachmenttype",
	"markclass",
	"nameid",
	"null",
	"parameters",
	"pos",
	"position",
	"required",
	"righttoleft",
	"reversesub",
	"rsub",
	"script",
	"sub",
	"substitute",
	"subtable",
	"table",
	"usemarkfilteringset",
	"useextension",
	"valuerecorddef",
	"base",
	"gdef",
	"head",
	"hhea",
	"name",
	"vhea",
	"vmtx",
];

/// `asFea(g)` for a glyph name string: keywords are escaped with a backslash.
pub fn glyph_as_fea(g: &str) -> String {
	if FEA_KEYWORDS.contains(&g.to_lowercase().as_str()) {
		format!("\\{g}")
	} else {
		g.to_owned()
	}
}

/// `asFea((start, end))` for a glyph range.
pub fn range_as_fea(start: &str, end: &str) -> String {
	format!("{} - {}", glyph_as_fea(start), glyph_as_fea(end))
}

/// A device table: `(ppem size, delta)` pairs.
pub type Device = Vec<(i64, i64)>;

/// `deviceToString(device)`.
pub fn device_to_string(device: Option<&Device>) -> String {
	match device {
		None => "<device NULL>".to_owned(),
		Some(d) => {
			let items: Vec<String> = d.iter().map(|(a, b)| format!("{a} {b}")).collect();
			format!("<device {}>", items.join(", "))
		}
	}
}

/// Python truthiness of an optional device table (`if device:`).
pub(crate) fn device_truthy(device: Option<&Device>) -> bool {
	device.is_some_and(|d| !d.is_empty())
}
