//! Building OpenType tables from feature files (`fontTools.feaLib.builder`).
//!
//! [`add_open_type_features`] and [`add_open_type_features_from_string`] parse a
//! feature file and build its tables into a font with a [`Builder`]. The
//! callbacks that `ast` nodes call in Python's `build()` methods are methods of
//! [`Builder`]; walking the tree is [`Builder::build_statements`].

mod ast_build;
mod tables;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use fontoxide_misc::Tag;
use fontoxide_misc::py::str_repr;
use fontoxide_otllib::GlyphMap;
use fontoxide_otllib::builder::{
	CaretCoord, ChainContextualRule, ChainTarget, LayoutTable, LookupBuilder, LookupKind,
	LookupRef, MapKey, RuleLookup, build_anchor, build_device,
};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::conv::ValueRecord as OtValueRecord;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Avar, Fvar};
use fontoxide_varlib::builder::build_var_dev_table;
use fontoxide_varlib::feature_vars::AxisBox;
use fontoxide_varlib::models::{normalize_value, piecewise_linear_map};
use fontoxide_varlib::var_store::OnlineVarStoreBuilder;
use indexmap::{IndexMap, IndexSet};

use crate::ast::{
	Anchor, AnchorMarks, BaseMinMax, BaseScriptRecord, ChainLookups, Device, FeatureFile,
	IntOrFloat, OS2Value, STATAxisValueStatement, STATDesignAxisStatement, STATNameStatement,
	ScalarValue, ValueRecord,
};
use crate::error::{Error, Result};
use crate::lexer::FeatureSource;
use crate::location::{FeatureLibLocation, Location};
use crate::lookup_debug_info::{LOOKUP_DEBUG_ENV_VAR, LookupDebugInfo};
use crate::parser::Parser;
use crate::variable_scalar::{VariableScalar, VariableScalarBuilder};

/// A `(script, language)` pair.
type LangSys = (String, String);

/// A `(script, language, feature)` key of the features.
type FeatureKey = (String, String, String);

/// The tables the builder can build (`Builder.supportedTables`).
pub const SUPPORTED_TABLES: [&str; 10] = [
	"BASE", "GDEF", "GPOS", "GSUB", "OS/2", "head", "hhea", "name", "vhea", "STAT",
];

/// A feature file to build: a path or text to parse, or a parsed tree (the
/// `featurefile` argument of `Builder` and `addOpenTypeFeatures`).
#[derive(Debug, Clone)]
pub enum FeatureInput {
	/// A path or file object to parse.
	Source(FeatureSource),
	/// A pre-parsed tree (`FeatureFile`).
	Tree(FeatureFile),
}

impl From<FeatureSource> for FeatureInput {
	fn from(source: FeatureSource) -> Self {
		FeatureInput::Source(source)
	}
}

impl From<FeatureFile> for FeatureInput {
	fn from(tree: FeatureFile) -> Self {
		FeatureInput::Tree(tree)
	}
}

impl From<PathBuf> for FeatureInput {
	fn from(path: PathBuf) -> Self {
		FeatureInput::Source(FeatureSource::Path(path))
	}
}

impl From<&Path> for FeatureInput {
	fn from(path: &Path) -> Self {
		FeatureInput::Source(FeatureSource::Path(path.to_path_buf()))
	}
}

/// Adds features from a file to a font (`addOpenTypeFeatures`). This replaces
/// any features currently present.
///
/// `featurefile` is a path or text to parse, or a parsed tree. `tables`
/// restricts the affected tables to those listed. `debug` adds source debugging
/// information to the font in the `Debg` table.
pub fn add_open_type_features(
	font: &mut Font,
	featurefile: impl Into<FeatureInput>,
	tables: Option<&[&str]>,
	debug: bool,
) -> Result<()> {
	let mut builder = Builder::new(font, featurefile)?;
	builder.build(tables, debug)
}

/// Adds features from a string to a font (`addOpenTypeFeaturesFromString`).
/// This replaces any features currently present.
///
/// The directory containing `filename` is the root of relative `include()`
/// paths; without it, the current directory is. `tables` and `debug` are as in
/// [`add_open_type_features`].
pub fn add_open_type_features_from_string(
	font: &mut Font,
	features: &str,
	filename: Option<&str>,
	tables: Option<&[&str]>,
	debug: bool,
) -> Result<()> {
	let featurefile = FeatureSource::Text {
		text: features.to_owned(),
		name: filename.filter(|f| !f.is_empty()).map(str::to_owned),
	};
	add_open_type_features(font, featurefile, tables, debug)
}

/// The name ID of a name record: a number, a feature tag (for `featureNames`
/// and `sizemenuname`), or a feature tag and block name (for `cvParameters`).
#[derive(Debug, Clone, PartialEq)]
enum NameKey {
	Id(i64),
	Feature(String),
	Cv(String, String),
}

/// A name record to add (`names_`).
#[derive(Debug, Clone)]
struct NameEntry {
	name_id: NameKey,
	platform_id: i64,
	plat_enc_id: i64,
	lang_id: i64,
	string: String,
}

/// A BASE axis: the baseline tags, the script records and the `MinMax` records.
type BaseAxisData = (Vec<String>, Vec<BaseScriptRecord>, Vec<BaseMinMax>);

/// The STAT table statements (`stat_`).
#[derive(Debug, Clone, Default)]
struct StatData {
	elided_fallback_name: Option<Vec<STATNameStatement>>,
	elided_fallback_name_id: Option<i64>,
	design_axes: Option<Vec<STATDesignAxisStatement>>,
	axis_value_records: Option<Vec<STATAxisValueStatement>>,
}

impl StatData {
	fn is_empty(&self) -> bool {
		self.elided_fallback_name.is_none()
			&& self.elided_fallback_name_id.is_none()
			&& self.design_axes.is_none()
			&& self.axis_value_records.is_none()
	}
}

/// The position of a lookup to build: a shared builder, or one that a
/// substitution builder was promoted to.
enum BuiltLookup {
	Shared(LookupRef),
	Owned(Box<LookupBuilder>),
}

/// Builds OpenType tables from a feature file (`Builder`).
///
/// The fields and methods ending in `_` are those of fontTools, which the
/// feature file statements call while the tree is walked.
#[derive(Debug)]
pub struct Builder<'a> {
	font: &'a mut Font,
	parse_tree: Option<FeatureFile>,
	file: Option<FeatureSource>,
	/// The glyph name to glyph ID map (`glyphMap`). A test may replace it to
	/// pretend the font has more than 65535 glyphs, as fontTools' tests patch
	/// the font.
	pub glyph_map: GlyphMap,
	varstorebuilder: Option<OnlineVarStoreBuilder>,
	axes: Vec<(String, (f64, f64, f64))>,
	scalar_builder: Option<VariableScalarBuilder>,
	default_language_systems_: IndexSet<LangSys>,
	/// The current script (`script_`).
	pub script_: Option<String>,
	/// The current language (`language_`).
	pub language_: Option<String>,
	/// The current lookup flags (`lookupflag_`).
	pub lookupflag_: u16,
	/// The current mark filtering set (`lookupflag_markFilterSet_`).
	pub lookupflag_mark_filter_set_: Option<u16>,
	use_extension_: bool,
	/// The language systems the current rules are registered under
	/// (`language_systems`), sorted.
	// LENIENT(L34): fontTools keeps a frozenset of string pairs, whose iteration order
	// (which decides the order features are first registered) depends on the
	// string hashes; here the set is sorted.
	pub language_systems: Vec<LangSys>,
	seen_non_dflt_script_: bool,
	named_lookups_: IndexMap<String, Option<LookupRef>>,
	cur_lookup_: Option<LookupRef>,
	cur_lookup_name_: Option<String>,
	cur_feature_name_: Option<String>,
	lookups_: Vec<LookupRef>,
	/// The debugging information of the lookups of `GSUB` and `GPOS`
	/// (`lookup_locations`).
	pub lookup_locations: IndexMap<String, IndexMap<String, LookupDebugInfo>>,
	features_: IndexMap<FeatureKey, Vec<LookupRef>>,
	/// The required feature of each language system (`required_features_`).
	pub required_features_: IndexMap<LangSys, String>,
	feature_variations_: IndexMap<FeatureKey, IndexMap<String, Vec<LookupRef>>>,
	aalt_features_: Vec<(Location, String)>,
	aalt_location_: Location,
	aalt_alternates_: IndexMap<String, Vec<String>>,
	aalt_use_extension_: bool,
	feature_names_: HashSet<String>,
	feature_names_ids_: HashMap<String, u16>,
	cv_parameters_: HashSet<String>,
	cv_parameters_ids_: HashMap<(String, String), u16>,
	/// The number of named parameters of each character variant feature
	/// (`cv_num_named_params_`).
	pub cv_num_named_params_: HashMap<String, i64>,
	cv_characters_: HashMap<String, Vec<i64>>,
	size_parameters_: Option<(f64, i64, f64, f64)>,
	font_revision_: Option<f64>,
	names_: Vec<NameEntry>,
	base_horiz_axis_: Option<BaseAxisData>,
	base_vert_axis_: Option<BaseAxisData>,
	attach_points_: IndexMap<String, IndexSet<i64>>,
	lig_caret_coords_: IndexMap<String, Vec<CaretCoord>>,
	lig_caret_points_: IndexMap<String, Vec<i64>>,
	glyph_class_defs_: IndexMap<String, (u16, Location)>,
	mark_attach_: IndexMap<String, (u16, Location)>,
	mark_attach_class_id_: HashMap<Vec<String>, u16>,
	mark_filter_sets_: IndexMap<Vec<String>, u16>,
	os2_: IndexMap<String, OS2Value>,
	hhea_: IndexMap<String, i64>,
	vhea_: IndexMap<String, i64>,
	stat_: StatData,
	/// The normalized condition sets by name (`conditionsets_`).
	pub conditionsets_: IndexMap<String, AxisBox>,
	merge_cache_: Option<MergeCache>,
}

/// `str(location)` for the otlLib builders.
fn otl_location(location: &Location) -> fontoxide_otllib::Location {
	location.as_ref().map(ToString::to_string)
}

/// `str(location)`, which is `None` for no location.
fn location_string(location: &Location) -> String {
	match location {
		Some(l) => l.to_string(),
		None => "None".to_owned(),
	}
}

/// A `FeatureLibError` with a location that fontTools has as a string: parsed
/// back if it is `file:line:column`, otherwise written before the message.
fn error_at(message: String, location: Option<&str>) -> Error {
	let Some(location) = location.filter(|l| !l.is_empty()) else {
		return Error::feature_lib(message, None);
	};
	let mut parts = location.rsplitn(3, ':');
	if let (Some(column), Some(line), Some(file)) = (parts.next(), parts.next(), parts.next())
		&& let (Ok(line), Ok(column)) = (line.parse(), column.parse())
	{
		return Error::feature_lib(message, Some(FeatureLibLocation::new(file, line, column)));
	}
	Error::feature_lib(format!("{location}: {message}"), None)
}

/// `FeatureLibError(str(e), e.location)` for an `OpenTypeLibError`, which
/// writes the location twice, as fontTools does.
fn wrap_otl_error(e: fontoxide_otllib::Error) -> Error {
	match &e {
		fontoxide_otllib::Error::OpenTypeLib { location, .. } => {
			let location = location.clone();
			error_at(e.to_string(), location.as_deref())
		}
		_ => Error::OtlLib(e),
	}
}

/// A value for a 16-bit signed field.
fn to_i16(v: i64) -> Result<i16> {
	i16::try_from(v).map_err(|_| {
		Error::Value(format!(
			"short format requires -32768 <= number <= 32767: {v}"
		))
	})
}

/// A value for a 16-bit unsigned field.
fn to_u16(v: i64) -> Result<u16> {
	u16::try_from(v)
		.map_err(|_| Error::Value(format!("ushort format requires 0 <= number <= 65535: {v}")))
}

/// A tag from a string, padded with spaces.
fn to_tag(s: &str) -> Result<Tag> {
	Tag::from_str_padded(s).ok_or_else(|| Error::Value(format!("invalid tag: {}", str_repr(s))))
}

/// `tuple(sorted(frozenset(glyphs)))`: the key of a glyph set.
fn glyph_set_key(glyphs: &[String]) -> Vec<String> {
	let set: BTreeSet<&String> = glyphs.iter().collect();
	set.into_iter().cloned().collect()
}

/// `repr()` of a tuple of glyph names.
fn tuple_repr(glyphs: &[String]) -> String {
	let items: Vec<String> = glyphs.iter().map(|g| str_repr(g)).collect();
	if items.len() == 1 {
		format!("({},)", items[0])
	} else {
		format!("({})", items.join(", "))
	}
}

impl<'a> Builder<'a> {
	/// `Builder(font, featurefile)`: a builder for a font. `featurefile` is a
	/// path or text to parse, or a parsed tree.
	pub fn new(font: &'a mut Font, featurefile: impl Into<FeatureInput>) -> Result<Self> {
		let (parse_tree, file) = match featurefile.into() {
			FeatureInput::Tree(tree) => (Some(tree), None),
			FeatureInput::Source(source) => (None, Some(source)),
		};
		let glyph_map = GlyphMap::from_font(font)?;
		let mut varstorebuilder = None;
		let mut axes = Vec::new();
		let mut scalar_builder = None;
		if let Some(fvar) = font.get::<Fvar>()? {
			axes = fvar
				.axes
				.iter()
				.map(|a| {
					(
						a.tag().to_string(),
						(a.min_value, a.default_value, a.max_value),
					)
				})
				.collect();
			let tags: Vec<String> = axes.iter().map(|(t, _)| t.clone()).collect();
			varstorebuilder = Some(OnlineVarStoreBuilder::new(tags)?);
			scalar_builder = Some(VariableScalarBuilder::from_ttf(font)?);
		}
		let mut lookup_locations = IndexMap::new();
		lookup_locations.insert("GSUB".to_owned(), IndexMap::new());
		lookup_locations.insert("GPOS".to_owned(), IndexMap::new());
		Ok(Builder {
			font,
			parse_tree,
			file,
			glyph_map,
			varstorebuilder,
			axes,
			scalar_builder,
			default_language_systems_: IndexSet::new(),
			script_: None,
			language_: None,
			lookupflag_: 0,
			lookupflag_mark_filter_set_: None,
			use_extension_: false,
			language_systems: Vec::new(),
			seen_non_dflt_script_: false,
			named_lookups_: IndexMap::new(),
			cur_lookup_: None,
			cur_lookup_name_: None,
			cur_feature_name_: None,
			lookups_: Vec::new(),
			lookup_locations,
			features_: IndexMap::new(),
			required_features_: IndexMap::new(),
			feature_variations_: IndexMap::new(),
			aalt_features_: Vec::new(),
			aalt_location_: None,
			aalt_alternates_: IndexMap::new(),
			aalt_use_extension_: false,
			feature_names_: HashSet::new(),
			feature_names_ids_: HashMap::new(),
			cv_parameters_: HashSet::new(),
			cv_parameters_ids_: HashMap::new(),
			cv_num_named_params_: HashMap::new(),
			cv_characters_: HashMap::new(),
			size_parameters_: None,
			font_revision_: None,
			names_: Vec::new(),
			base_horiz_axis_: None,
			base_vert_axis_: None,
			attach_points_: IndexMap::new(),
			lig_caret_coords_: IndexMap::new(),
			lig_caret_points_: IndexMap::new(),
			glyph_class_defs_: IndexMap::new(),
			mark_attach_: IndexMap::new(),
			mark_attach_class_id_: HashMap::new(),
			mark_filter_sets_: IndexMap::new(),
			os2_: IndexMap::new(),
			hhea_: IndexMap::new(),
			vhea_: IndexMap::new(),
			stat_: StatData::default(),
			conditionsets_: IndexMap::new(),
			merge_cache_: None,
		})
	}

	/// The font being built.
	pub fn font(&mut self) -> &mut Font {
		self.font
	}

	/// Whether the font has glyph IDs above 65535 (`font.hasExtendedGlyphIDs()`),
	/// as the glyph map tells.
	fn has_extended_glyph_ids(&self) -> bool {
		self.glyph_map.has_extended_glyph_ids()
	}

	/// Parses the feature file if needed, walks it and builds the tables
	/// (`build`). `tables` restricts the tables built; `debug` (or the
	/// `FONTTOOLS_LOOKUP_DEBUGGING` environment variable) adds the `Debg` table.
	pub fn build(&mut self, tables: Option<&[&str]>, debug: bool) -> Result<()> {
		if self.parse_tree.is_none() {
			let file = self
				.file
				.clone()
				.ok_or_else(|| Error::Value("no feature file".to_owned()))?;
			let names: Vec<String> = self.font.glyph_order()?.names().to_vec();
			self.parse_tree = Some(Parser::new(file, names)?.parse()?);
		}
		let tree = self.parse_tree.take().unwrap_or_default();
		let result = self.build_statements(&tree.statements);
		self.parse_tree = Some(tree);
		result?;
		// By default, build all the supported tables.
		let tables: BTreeSet<String> = match tables {
			None => SUPPORTED_TABLES.iter().map(|t| (*t).to_owned()).collect(),
			Some(tables) => {
				let tables: BTreeSet<String> = tables.iter().map(|t| (*t).to_owned()).collect();
				let unsupported: Vec<&String> = tables
					.iter()
					.filter(|t| !SUPPORTED_TABLES.contains(&t.as_str()))
					.collect();
				if !unsupported.is_empty() {
					let unsupported: Vec<&str> = unsupported.iter().map(|t| t.as_str()).collect();
					return Err(Error::NotImplemented(format!(
						"The following tables were requested but are unsupported: {}.",
						unsupported.join(", ")
					)));
				}
				tables
			}
		};
		let has = |t: &str| tables.contains(t);
		if has("GSUB") {
			self.build_feature_aalt_()?;
		}
		if has("head") {
			self.build_head()?;
		}
		if has("hhea") {
			self.build_hhea()?;
		}
		if has("vhea") {
			self.build_vhea()?;
		}
		if has("name") {
			self.build_name()?;
		}
		if has("OS/2") {
			self.build_os_2()?;
		}
		if has("STAT") {
			self.build_stat()?;
		}
		for tag in [LayoutTable::Gpos, LayoutTable::Gsub] {
			if !has(&tag.to_string()) {
				continue;
			}
			self.build_layout_table(tag)?;
		}
		if (self.font.contains(Tag::new(b"GPOS")) || self.font.contains(Tag::new(b"GSUB")))
			&& let Some(_) = self.font.get::<fontoxide_ttlib::tables::Os2>()?
		{
			let max_context = fontoxide_otllib::max_context_calc::max_ctx_font(self.font)?;
			if let Some(os2) = self.font.get::<fontoxide_ttlib::tables::Os2>()? {
				os2.us_max_context = u16::try_from(max_context).unwrap_or(u16::MAX);
			}
		}
		if has("GDEF") {
			match self.build_gdef()? {
				Some(gdef) => self.font.insert(Box::new(gdef)),
				None => {
					self.font.remove(Tag::new(b"GDEF"));
				}
			}
		}
		if has("BASE") {
			match self.build_base()? {
				Some(base) => self.font.insert(Box::new(base)),
				None => {
					self.font.remove(Tag::new(b"BASE"));
				}
			}
		}
		let env = std::env::var_os(LOOKUP_DEBUG_ENV_VAR).is_some_and(|v| !v.is_empty());
		if debug || env {
			self.build_debg();
		}
		Ok(())
	}

	/// A new lookup builder with the current flags.
	fn new_lookup(&self, kind: LookupKind, location: &Location) -> LookupRef {
		let mut builder = LookupBuilder::new(kind, self.glyph_map.clone(), otl_location(location));
		let common = builder.common_mut();
		common.lookupflag = self.lookupflag_;
		common.mark_filter_set = self.lookupflag_mark_filter_set_;
		common.extension = self.use_extension_;
		builder.into_ref()
	}

	/// A lookup for a contextual rule to refer to (`get_chained_lookup_`).
	fn get_chained_lookup_(&mut self, location: &Location, kind: LookupKind) -> LookupRef {
		let result = self.new_lookup(kind, location);
		self.lookups_.push(result.clone());
		result
	}

	/// `add_lookup_to_feature_`.
	fn add_lookup_to_feature_(&mut self, lookup: &LookupRef, feature_name: &str) {
		for (script, lang) in &self.language_systems {
			let key = (script.clone(), lang.clone(), feature_name.to_owned());
			self.features_.entry(key).or_default().push(lookup.clone());
		}
	}

	/// The current lookup if a rule of `kind` can be added to it, or a new
	/// lookup (`get_lookup_`).
	fn get_lookup_(
		&mut self,
		location: &Location,
		kind: LookupKind,
		mapping: Option<&IndexMap<Vec<String>, Vec<String>>>,
	) -> Result<LookupRef> {
		if let Some(cur) = &self.cur_lookup_ {
			let b = cur.borrow();
			if b.kind() == kind
				&& b.common().lookupflag == self.lookupflag_
				&& b.common().mark_filter_set == self.lookupflag_mark_filter_set_
				&& b.can_add_mapping(mapping)
			{
				return Ok(cur.clone());
			}
		}
		if self.cur_lookup_name_.is_some() && self.cur_lookup_.is_some() {
			return Err(Error::feature_lib(
				"Within a named lookup block, all rules must be of the same lookup type and flag",
				location.clone(),
			));
		}
		let lookup = self.new_lookup(kind, location);
		self.cur_lookup_ = Some(lookup.clone());
		self.lookups_.push(lookup.clone());
		if let Some(name) = &self.cur_lookup_name_ {
			// We are starting a lookup rule inside a named lookup block.
			self.named_lookups_
				.insert(name.clone(), Some(lookup.clone()));
		}
		if let Some(feature) = self.cur_feature_name_.clone() {
			// We are starting a lookup rule inside a feature. This includes
			// lookup rules inside named lookups inside features.
			self.add_lookup_to_feature_(&lookup, &feature);
		}
		Ok(lookup)
	}

	/// Builds the `aalt` feature from the features it refers to
	/// (`build_feature_aalt_`).
	fn build_feature_aalt_(&mut self) -> Result<()> {
		if self.aalt_features_.is_empty() && self.aalt_alternates_.is_empty() {
			return Ok(());
		}
		// > alternate glyphs will be sorted in the order that the source features
		// > are named in the aalt definition, not the order of the feature
		// > definitions in the file. Alternates defined explicitly ... will precede
		// > all others.
		// https://github.com/fonttools/fonttools/issues/836
		let mut alternates: IndexMap<String, Vec<String>> = self.aalt_alternates_.clone();
		let mut names = self.aalt_features_.clone();
		names.push((None, "aalt".to_owned()));
		for (location, name) in &names {
			let feature: Vec<&Vec<LookupRef>> = self
				.features_
				.iter()
				.filter(|((_, _, f), _)| f == name)
				.map(|(_, lookups)| lookups)
				.collect();
			// "aalt" does not have to specify its own lookups, but it might.
			if feature.is_empty() && name != "aalt" {
				log::warn!(
					"{}: Feature {name} has not been defined",
					location_string(location)
				);
				continue;
			}
			for lookups in feature {
				for lookup in lookups {
					let alts = lookup.borrow().get_alternate_glyphs()?;
					for (glyph, alts) in alts {
						let alts_for_glyph = alternates.entry(glyph).or_default();
						for g in alts {
							if !alts_for_glyph.contains(&g) {
								alts_for_glyph.push(g);
							}
						}
					}
				}
			}
		}
		let single: IndexMap<MapKey<String>, String> = alternates
			.iter()
			.filter(|(_, repl)| repl.len() == 1)
			.map(|(glyph, repl)| (MapKey::Key(glyph.clone()), repl[0].clone()))
			.collect();
		let multi: IndexMap<MapKey<String>, Vec<String>> = alternates
			.iter()
			.filter(|(_, repl)| repl.len() > 1)
			.map(|(glyph, repl)| (MapKey::Key(glyph.clone()), repl.clone()))
			.collect();
		if single.is_empty() && multi.is_empty() {
			return Ok(());
		}
		self.features_
			.retain(|(_, _, feature), _| feature != "aalt");
		let old_lookups = std::mem::take(&mut self.lookups_);
		let aalt_location = self.aalt_location_.clone();
		self.start_feature(&aalt_location, "aalt", self.aalt_use_extension_)?;
		// fontTools uses the location of the loop above, which ends at None.
		if !single.is_empty() {
			let lookup = self.get_lookup_(&None, LookupKind::SingleSubst, None)?;
			if let LookupBuilder::SingleSubst(b) = &mut *lookup.borrow_mut() {
				b.mapping = single;
			}
		}
		if !multi.is_empty() {
			let lookup = self.get_lookup_(&None, LookupKind::AlternateSubst, None)?;
			if let LookupBuilder::AlternateSubst(b) = &mut *lookup.borrow_mut() {
				b.alternates = multi;
			}
		}
		self.end_feature()?;
		self.lookups_.extend(old_lookups);
		Ok(())
	}

	/// The name of a named lookup, by builder (`get_lookup_name_`), for all
	/// lookups at once.
	fn lookup_names(&self) -> HashMap<*const RefCell<LookupBuilder>, String> {
		let mut rev = HashMap::new();
		for (name, lookup) in &self.named_lookups_ {
			if let Some(lookup) = lookup {
				rev.insert(Rc::as_ptr(lookup), name.clone());
			}
		}
		rev
	}

	/// Builds the lookups of `GSUB` or `GPOS` (`buildLookups_`).
	fn build_lookups_(&mut self, tag: LayoutTable) -> Result<Vec<Option<Box<ot::Lookup>>>> {
		for lookup in &self.lookups_ {
			lookup.borrow_mut().common_mut().lookup_index = None;
		}
		let names = self.lookup_names();
		let tag_name = tag.to_string();
		let mut lookups: Vec<BuiltLookup> = Vec::new();
		for lookup in self.lookups_.clone() {
			if lookup.borrow().common().table != tag {
				continue;
			}
			let name = names.get(&Rc::as_ptr(&lookup)).cloned();
			let resolved: Option<Vec<BuiltLookup>> = match &*lookup.borrow() {
				LookupBuilder::AnySubst(b) => b.promote_lookup_type(name.is_some())?.map(|v| {
					v.into_iter()
						.map(|b| BuiltLookup::Owned(Box::new(b)))
						.collect()
				}),
				_ => Some(vec![BuiltLookup::Shared(lookup.clone())]),
			};
			let location = lookup.borrow().common().location.clone();
			let Some(resolved) = resolved else {
				return Err(error_at(
					"Within a named lookup block, all rules must be of the same lookup type and flag"
						.to_owned(),
					location.as_deref(),
				));
			};
			for l in resolved {
				let index = lookups.len();
				lookup.borrow_mut().common_mut().lookup_index = Some(index);
				if let Some(locations) = self.lookup_locations.get_mut(&tag_name) {
					locations.insert(
						index.to_string(),
						LookupDebugInfo {
							location: location.clone().unwrap_or_else(|| "None".to_owned()),
							name: name.clone(),
							feature: None,
						},
					);
				}
				lookups.push(l);
			}
		}
		let mut ot_lookups = Vec::with_capacity(lookups.len());
		for (index, l) in lookups.into_iter().enumerate() {
			let built = match l {
				BuiltLookup::Shared(l) => l.borrow_mut().build(self.font),
				BuiltLookup::Owned(mut l) => l.build(self.font),
			};
			match built {
				Ok(lookup) => ot_lookups.push(lookup.map(Box::new)),
				Err(e @ fontoxide_otllib::Error::OpenTypeLib { .. }) => {
					return Err(wrap_otl_error(e));
				}
				// LENIENT(L34): fontTools looks the location up by the failing
				// builder's `lookup_index`, which a builder that a substitution
				// lookup was promoted to does not have (a KeyError); this uses
				// the position in the lookup list.
				Err(e) => {
					let location = self
						.lookup_locations
						.get(&tag_name)
						.and_then(|l| l.get(&index.to_string()))
						.map(|info| info.location.clone());
					return Err(error_at(e.to_string(), location.as_deref()));
				}
			}
		}
		Ok(ot_lookups)
	}

	/// Whether a feature has variations with lookups in the table
	/// (`any_feature_variations`).
	fn any_feature_variations(&self, feature_tag: &str, table: LayoutTable) -> bool {
		self.feature_variations_
			.iter()
			.filter(|((_, _, feature), _)| feature == feature_tag)
			.any(|(_, variations)| {
				variations
					.values()
					.any(|builders| builders.iter().any(|b| b.borrow().common().table == table))
			})
	}

	/// Builds the script, feature and lookup lists of `GSUB` or `GPOS`
	/// (`makeTable`).
	fn make_table(
		&mut self,
		tag: LayoutTable,
	) -> Result<(ot::ScriptList, ot::FeatureList, ot::LookupList)> {
		let lookup = self.build_lookups_(tag)?;
		let mut feature_list = ot::FeatureList::default();
		// Build a table for mapping (tag, lookup_indices) to feature_index. For
		// example, ('liga', (2,3,7)) --> 23.
		let mut feature_indices: HashMap<(String, Vec<usize>), usize> = HashMap::new();
		let mut required_feature_indices: HashMap<LangSys, usize> = HashMap::new();
		// 'latn' --> {'DEU': [23, 24]} for feature #23,24
		let mut scripts: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
		// Sort the feature table by feature tag:
		// https://github.com/fonttools/fonttools/issues/568
		let mut features: Vec<(&FeatureKey, &Vec<LookupRef>)> = self.features_.iter().collect();
		features.sort_by(|a, b| (&a.0.2, &a.0.1, &a.0.0).cmp(&(&b.0.2, &b.0.1, &b.0.0)));
		let tag_name = tag.to_string();
		let mut feature_records: Vec<(String, Vec<usize>)> = Vec::new();
		let mut debug_updates: Vec<(usize, FeatureKey)> = Vec::new();
		for (key, lookups) in features {
			let (script, lang, feature_tag) = key;
			// l.lookup_index will be None when a lookup is not needed for the
			// table under construction. For example, substitution rules will
			// have no lookup_index while building GPOS tables. We also
			// deduplicate lookup indices, as they only get applied once within a
			// given feature: https://github.com/fonttools/fonttools/issues/2946
			let mut lookup_indices: Vec<usize> = Vec::new();
			for l in lookups {
				if let Some(index) = l.borrow().common().lookup_index
					&& !lookup_indices.contains(&index)
				{
					lookup_indices.push(index);
				}
			}
			// Order doesn't matter, but lookup_indices preserves it. We want to
			// combine identical sets of lookups (order doesn't matter) but also
			// respect the order provided by the user.
			let mut lookup_key = lookup_indices.clone();
			lookup_key.sort_unstable();
			let size_feature = tag == LayoutTable::Gpos && feature_tag == "size";
			let force_feature = self.any_feature_variations(feature_tag, tag);
			if lookup_indices.is_empty() && !size_feature && !force_feature {
				continue;
			}
			for ix in &lookup_indices {
				debug_updates.push((*ix, key.clone()));
			}
			let feature_key = (feature_tag.clone(), lookup_key);
			let feature_index = match feature_indices.get(&feature_key) {
				Some(&index) => index,
				None => {
					let index = feature_records.len();
					feature_records.push((feature_tag.clone(), lookup_indices.clone()));
					feature_indices.insert(feature_key, index);
					index
				}
			};
			scripts
				.entry(script.clone())
				.or_default()
				.entry(lang.clone())
				.or_default()
				.push(feature_index);
			if self.required_features_.get(&(script.clone(), lang.clone())) == Some(feature_tag) {
				required_feature_indices.insert((script.clone(), lang.clone()), feature_index);
			}
		}
		if let Some(locations) = self.lookup_locations.get_mut(&tag_name) {
			for (ix, key) in debug_updates {
				match locations.get_mut(&ix.to_string()) {
					Some(info) => info.feature = Some(vec![key.0, key.1, key.2]),
					None => log::warn!(
						"feaLib.Builder subclass needs upgrading to stash debug information. See fonttools#2065."
					),
				}
			}
		}
		for (feature_tag, lookup_indices) in feature_records {
			let feature_params = self.build_feature_params(&feature_tag)?;
			let lookup_list_index = lookup_indices
				.iter()
				.map(|&i| to_u16(i as i64))
				.collect::<Result<Vec<u16>>>()?;
			feature_list.feature_record.push(ot::FeatureRecord {
				feature_tag: to_tag(&feature_tag)?,
				feature: Some(Box::new(ot::Feature {
					feature_params: feature_params.map(Box::new),
					lookup_count: Some(lookup_list_index.len() as u32),
					lookup_list_index,
				})),
			});
		}
		// Build ScriptList.
		let mut script_list = ot::ScriptList::default();
		for (script, lang_features) in scripts {
			let mut srec_script = ot::Script::default();
			for (lang, feature_indices) in lang_features {
				let req_feature_index = required_feature_indices
					.get(&(script.clone(), lang.clone()))
					.copied();
				let feature_index = feature_indices
					.iter()
					.filter(|&&i| Some(i) != req_feature_index)
					.map(|&i| to_u16(i as i64))
					.collect::<Result<Vec<u16>>>()?;
				let lang_sys = ot::LangSys {
					lookup_order: None,
					req_feature_index: match req_feature_index {
						Some(i) => to_u16(i as i64)?,
						None => 0xFFFF,
					},
					feature_count: Some(feature_index.len() as u32),
					feature_index,
				};
				if lang == "dflt" {
					srec_script.default_lang_sys = Some(Box::new(lang_sys));
				} else {
					srec_script.lang_sys_record.push(ot::LangSysRecord {
						lang_sys_tag: to_tag(&lang)?,
						lang_sys: Some(Box::new(lang_sys)),
					});
				}
			}
			srec_script.lang_sys_count = Some(srec_script.lang_sys_record.len() as u32);
			script_list.script_record.push(ot::ScriptRecord {
				script_tag: to_tag(&script)?,
				script: Some(Box::new(srec_script)),
			});
		}
		script_list.script_count = Some(script_list.script_record.len() as u32);
		feature_list.feature_count = Some(feature_list.feature_record.len() as u32);
		let lookup_list = ot::LookupList {
			lookup_count: Some(lookup.len() as u32),
			lookup,
		};
		Ok((script_list, feature_list, lookup_list))
	}

	/// The feature variations of a table: for each feature tag, the condition
	/// sets and the indices of their lookups in the table, if any lookup is in
	/// the table (part of `makeFeatureVariations`).
	#[allow(clippy::type_complexity)]
	fn feature_variations_for(
		&self,
		table: LayoutTable,
	) -> Result<Option<IndexMap<String, Vec<(AxisBox, Vec<u16>)>>>> {
		let mut feature_vars: IndexMap<String, Vec<(AxisBox, Vec<u16>)>> = IndexMap::new();
		let mut has_any_variations = false;
		// Sort out which lookups to build, gather their indices.
		for ((_, _, feature_tag), variations) in &self.feature_variations_ {
			let mut list = Vec::new();
			for (conditionset, builders) in variations {
				let raw_conditionset = self
					.conditionsets_
					.get(conditionset)
					.ok_or_else(|| Error::Value(format!("KeyError: {}", str_repr(conditionset))))?
					.clone();
				let mut indices = Vec::new();
				for b in builders {
					let b = b.borrow();
					if b.common().table != table {
						continue;
					}
					let index = b
						.common()
						.lookup_index
						.ok_or_else(|| Error::Assertion("b.lookup_index is not None".to_owned()))?;
					indices.push(to_u16(index as i64)?);
					has_any_variations = true;
				}
				list.push((raw_conditionset, indices));
			}
			feature_vars.insert(feature_tag.clone(), list);
		}
		Ok(has_any_variations.then_some(feature_vars))
	}

	/// Builds `GSUB` or `GPOS` and puts it in the font, or removes it if it is
	/// empty (part of `build`).
	fn build_layout_table(&mut self, tag: LayoutTable) -> Result<()> {
		let (script_list, feature_list, lookup_list) = self.make_table(tag)?;
		let feature_vars = if self.feature_variations_.is_empty() {
			None
		} else {
			self.feature_variations_for(tag)?
		};
		let extended = self.has_extended_glyph_ids();
		macro_rules! finish {
			($ty:ident, $table:ident, $tag:literal) => {{
				let mut table = ot::$ty {
					version: 0x0001_0000,
					script_list: Some(Box::new(script_list)),
					feature_list: Some(Box::new(feature_list)),
					lookup_list: Some(Box::new(lookup_list)),
					..Default::default()
				};
				if let Some(feature_vars) = feature_vars {
					for (feature_tag, conditions_and_lookups) in &feature_vars {
						fontoxide_varlib::feature_vars::add_feature_variations_raw(
							self.font,
							&mut table,
							conditions_and_lookups,
							&[to_tag(feature_tag)?],
						)?;
					}
				}
				// promoteLayoutTable_
				if extended {
					table.version = 0x0001_0002;
					table.script_list2 = table.script_list.take();
					table.feature_list2 = table.feature_list.take();
					table.lookup_list2 = table.lookup_list.take().map(|l| {
						Box::new(ot::LookupList2 {
							lookup_count: l.lookup_count,
							lookup: l.lookup,
						})
					});
				}
				let script_count = table
					.script_list
					.as_ref()
					.or(table.script_list2.as_ref())
					.map_or(0, |l| l.script_record.len());
				let feature_count = table
					.feature_list
					.as_ref()
					.or(table.feature_list2.as_ref())
					.map_or(0, |l| l.feature_record.len());
				let lookup_count = match (&table.lookup_list, &table.lookup_list2) {
					(Some(l), _) => l.lookup.len(),
					(None, Some(l)) => l.lookup.len(),
					(None, None) => 0,
				};
				if script_count > 0 || feature_count > 0 || lookup_count > 0 {
					self.font
						.insert(Box::new(fontoxide_ttlib::tables::$table { table }));
				} else {
					self.font.remove(Tag::new($tag));
				}
			}};
		}
		match tag {
			LayoutTable::Gpos => finish!(GPOS, Gpos, b"GPOS"),
			LayoutTable::Gsub => finish!(GSUB, Gsub, b"GSUB"),
		}
		Ok(())
	}

	/// Registers a `languagesystem` statement (`add_language_system`).
	pub fn add_language_system(
		&mut self,
		location: &Location,
		script: &str,
		language: &str,
	) -> Result<()> {
		// OpenType Feature File Specification, section 4.b.i
		if script == "DFLT" && language == "dflt" && !self.default_language_systems_.is_empty() {
			return Err(Error::feature_lib(
				"If \"languagesystem DFLT dflt\" is present, it must be the first of the languagesystem statements",
				location.clone(),
			));
		}
		if script == "DFLT" {
			if self.seen_non_dflt_script_ {
				return Err(Error::feature_lib(
					"languagesystems using the \"DFLT\" script tag must precede all other languagesystems",
					location.clone(),
				));
			}
		} else {
			self.seen_non_dflt_script_ = true;
		}
		let key = (script.to_owned(), language.to_owned());
		if self.default_language_systems_.contains(&key) {
			return Err(Error::feature_lib(
				format!(
					"\"languagesystem {} {}\" has already been specified",
					fontoxide_misc::py::strip(script),
					fontoxide_misc::py::strip(language)
				),
				location.clone(),
			));
		}
		self.default_language_systems_.insert(key);
		Ok(())
	}

	/// The language systems of the `languagesystem` statements, or `DFLT dflt`
	/// if there are none (`get_default_language_systems_`).
	fn get_default_language_systems_(&self) -> Vec<LangSys> {
		// OpenType Feature File specification, 4.b.i. languagesystem: If no
		// "languagesystem" statement is present, then the implementation must
		// behave exactly as though the following statement were present at the
		// beginning of the feature file: languagesystem DFLT dflt;
		if self.default_language_systems_.is_empty() {
			vec![("DFLT".to_owned(), "dflt".to_owned())]
		} else {
			let mut systems: Vec<LangSys> =
				self.default_language_systems_.iter().cloned().collect();
			systems.sort();
			systems
		}
	}

	/// Starts a feature block (`start_feature`).
	pub fn start_feature(
		&mut self,
		location: &Location,
		name: &str,
		use_extension: bool,
	) -> Result<()> {
		if use_extension && name != "aalt" {
			return Err(Error::feature_lib(
				"'useExtension' keyword for feature blocks is allowed only for 'aalt' feature",
				location.clone(),
			));
		}
		self.language_systems = self.get_default_language_systems_();
		// The current script and language start out as the first declared
		// language system, sorted by (script, language) tag. Kind of odd, but
		// matches makeotf (and fea-rs matches us).
		let first = self.language_systems.iter().min().cloned();
		self.script_ = first.as_ref().map(|f| f.0.clone());
		self.language_ = first.map(|f| f.1);
		self.cur_lookup_ = None;
		self.cur_feature_name_ = Some(name.to_owned());
		self.lookupflag_ = 0;
		self.lookupflag_mark_filter_set_ = None;
		self.use_extension_ = use_extension;
		if name == "aalt" {
			self.aalt_location_ = location.clone();
			self.aalt_use_extension_ = use_extension;
		}
		Ok(())
	}

	/// Ends a feature block (`end_feature`).
	pub fn end_feature(&mut self) -> Result<()> {
		if self.cur_feature_name_.is_none() {
			return Err(Error::Assertion(
				"self.cur_feature_name_ is not None".to_owned(),
			));
		}
		self.cur_feature_name_ = None;
		self.language_systems = Vec::new();
		self.script_ = None;
		self.language_ = None;
		self.cur_lookup_ = None;
		self.lookupflag_ = 0;
		self.lookupflag_mark_filter_set_ = None;
		self.use_extension_ = false;
		Ok(())
	}

	/// Starts a named lookup block (`start_lookup_block`).
	pub fn start_lookup_block(
		&mut self,
		location: &Location,
		name: &str,
		use_extension: bool,
	) -> Result<()> {
		if self.named_lookups_.contains_key(name) {
			return Err(Error::feature_lib(
				format!("Lookup \"{name}\" has already been defined"),
				location.clone(),
			));
		}
		if self.cur_feature_name_.as_deref() == Some("aalt") {
			return Err(Error::feature_lib(
				"Lookup blocks cannot be placed inside 'aalt' features; move it out, and then refer to it with a lookup statement",
				location.clone(),
			));
		}
		self.cur_lookup_name_ = Some(name.to_owned());
		self.named_lookups_.insert(name.to_owned(), None);
		self.cur_lookup_ = None;
		self.use_extension_ = use_extension;
		if self.cur_feature_name_.is_none() {
			self.lookupflag_ = 0;
			self.lookupflag_mark_filter_set_ = None;
		}
		Ok(())
	}

	/// Ends a named lookup block (`end_lookup_block`).
	pub fn end_lookup_block(&mut self) -> Result<()> {
		if self.cur_lookup_name_.is_none() {
			return Err(Error::Assertion(
				"self.cur_lookup_name_ is not None".to_owned(),
			));
		}
		self.cur_lookup_name_ = None;
		self.cur_lookup_ = None;
		self.use_extension_ = false;
		if self.cur_feature_name_.is_none() {
			self.lookupflag_ = 0;
			self.lookupflag_mark_filter_set_ = None;
		}
		Ok(())
	}

	/// Adds a named lookup to the current feature (`add_lookup_call`).
	pub fn add_lookup_call(&mut self, lookup_name: &str) -> Result<()> {
		let Some(lookup) = self.named_lookups_.get(lookup_name) else {
			return Err(Error::Assertion(lookup_name.to_owned()));
		};
		let lookup = lookup.clone();
		self.cur_lookup_ = None;
		// Skip empty named lookups.
		if let Some(lookup) = lookup {
			let feature = self
				.cur_feature_name_
				.clone()
				.unwrap_or_else(|| "None".to_owned());
			self.add_lookup_to_feature_(&lookup, &feature);
		}
		Ok(())
	}

	/// `set_font_revision`.
	pub fn set_font_revision(&mut self, _location: &Location, revision: f64) {
		self.font_revision_ = Some(revision);
	}

	/// A `language` statement (`set_language`).
	pub fn set_language(
		&mut self,
		location: &Location,
		languages: &[String],
		include_default: bool,
		required: bool,
	) -> Result<()> {
		if languages.is_empty() || !languages.iter().all(|l| l.chars().count() == 4) {
			return Err(Error::Assertion(
				"languages and all(len(language) == 4 for language in languages)".to_owned(),
			));
		}
		// Repeated tags in a single statement do not designate distinct language
		// systems.
		let languages: Vec<String> = languages
			.iter()
			.cloned()
			.collect::<IndexSet<String>>()
			.into_iter()
			.collect();
		if let Some(name @ ("aalt" | "size")) = self.cur_feature_name_.as_deref() {
			return Err(Error::feature_lib(
				format!("Language statements are not allowed within \"feature {name}\""),
				location.clone(),
			));
		}
		let Some(feature) = self.cur_feature_name_.clone() else {
			return Err(Error::feature_lib(
				"Language statements are not allowed within standalone lookup blocks",
				location.clone(),
			));
		};
		self.cur_lookup_ = None;
		let script = self.script_.clone().unwrap_or_else(|| "None".to_owned());
		if required {
			for language in &languages {
				let key = (script.clone(), language.clone());
				if let Some(existing) = self.required_features_.get(&key) {
					return Err(Error::feature_lib(
						format!(
							"Language {} (script {}) has already specified feature {} as its required feature",
							fontoxide_misc::py::strip(language),
							fontoxide_misc::py::strip(&script),
							fontoxide_misc::py::strip(existing)
						),
						location.clone(),
					));
				}
			}
		}
		for language in &languages {
			let key = (script.clone(), language.clone(), feature.clone());
			let lookups: Vec<LookupRef> = self
				.features_
				.get(&(script.clone(), "dflt".to_owned(), feature.clone()))
				.cloned()
				.unwrap_or_default();
			// Only ever add or remove the default lookups, never replace the
			// list: duplicate script/lang statements are allowed, and must not
			// drop the lookups already registered for this language system; see
			// https://github.com/fonttools/fonttools/issues/3748
			let cur_lookups = self.features_.entry(key).or_default();
			if language == "dflt" || include_default {
				let add: Vec<LookupRef> = lookups
					.iter()
					.filter(|x| !cur_lookups.iter().any(|c| Rc::ptr_eq(c, x)))
					.cloned()
					.collect();
				cur_lookups.extend(add);
			} else {
				cur_lookups.retain(|x| !lookups.iter().any(|l| Rc::ptr_eq(l, x)));
			}
			if required {
				self.required_features_
					.insert((script.clone(), language.clone()), feature.clone());
			}
		}
		let mut systems: Vec<LangSys> = languages
			.iter()
			.map(|l| (script.clone(), l.clone()))
			.collect();
		systems.sort();
		systems.dedup();
		self.language_systems = systems;
		self.language_ = if languages.len() == 1 {
			Some(languages[0].clone())
		} else {
			None
		};
		Ok(())
	}

	/// The mark attachment class of a set of glyphs, made up if new
	/// (`getMarkAttachClass_`).
	fn get_mark_attach_class_(&mut self, location: &Location, glyphs: &[String]) -> Result<u16> {
		let glyphs = glyph_set_key(glyphs);
		if let Some(&id) = self.mark_attach_class_id_.get(&glyphs) {
			return Ok(id);
		}
		let id = to_u16(self.mark_attach_class_id_.len() as i64 + 1)?;
		self.mark_attach_class_id_.insert(glyphs.clone(), id);
		// LENIENT(L34): fontTools iterates a frozenset, so which glyph an error names
		// depends on the string hashes; here the glyphs are in sorted order.
		for glyph in glyphs {
			if let Some((_, loc)) = self.mark_attach_.get(&glyph) {
				return Err(Error::feature_lib(
					format!(
						"Glyph {glyph} already has been assigned a MarkAttachmentType at {}",
						location_string(loc)
					),
					location.clone(),
				));
			}
			self.mark_attach_.insert(glyph, (id, location.clone()));
		}
		Ok(id)
	}

	/// The mark filtering set of a set of glyphs, made up if new
	/// (`getMarkFilterSet_`).
	fn get_mark_filter_set_(&mut self, glyphs: &[String]) -> Result<u16> {
		let glyphs = glyph_set_key(glyphs);
		if let Some(&id) = self.mark_filter_sets_.get(&glyphs) {
			return Ok(id);
		}
		let id = to_u16(self.mark_filter_sets_.len() as i64)?;
		self.mark_filter_sets_.insert(glyphs, id);
		Ok(id)
	}

	/// A `lookupflag` statement (`set_lookup_flag`).
	pub fn set_lookup_flag(
		&mut self,
		location: &Location,
		value: i64,
		mark_attach: Option<&[String]>,
		mark_filter: Option<&[String]>,
	) -> Result<()> {
		let mut value = value & 0xFF;
		if let Some(mark_attach) = mark_attach {
			let mark_attach_class = self.get_mark_attach_class_(location, mark_attach)?;
			value |= i64::from(mark_attach_class) << 8;
		}
		if let Some(mark_filter) = mark_filter {
			let mark_filter_set = self.get_mark_filter_set_(mark_filter)?;
			value |= 0x10;
			self.lookupflag_mark_filter_set_ = Some(mark_filter_set);
		} else {
			self.lookupflag_mark_filter_set_ = None;
		}
		self.lookupflag_ = to_u16(value)?;
		Ok(())
	}

	/// A `script` statement (`set_script`).
	pub fn set_script(&mut self, location: &Location, script: &str) -> Result<()> {
		if let Some(name @ ("aalt" | "size")) = self.cur_feature_name_.as_deref() {
			return Err(Error::feature_lib(
				format!("Script statements are not allowed within \"feature {name}\""),
				location.clone(),
			));
		}
		if self.cur_feature_name_.is_none() {
			return Err(Error::feature_lib(
				"Script statements are not allowed within standalone lookup blocks",
				location.clone(),
			));
		}
		let dflt = ["dflt".to_owned()];
		if self.script_.as_deref() == Some(script) && self.language_.as_deref() == Some("dflt") {
			// A script statement naming the already-current script still narrows
			// the language systems that the rules after it are registered under,
			// and still terminates the current lookup; it differs from the normal
			// path only in leaving script_ and the lookupflag alone, see
			// https://github.com/fonttools/fonttools/issues/1824.
			return self.set_language(location, &dflt, true, false);
		}
		self.cur_lookup_ = None;
		self.script_ = Some(script.to_owned());
		self.lookupflag_ = 0;
		self.lookupflag_mark_filter_set_ = None;
		self.set_language(location, &dflt, true, false)
	}

	/// The lookups named by the lookup lists of a contextual rule
	/// (`find_lookup_builders_`).
	fn find_lookup_builders_(
		&self,
		location: &Location,
		lookups: &ChainLookups,
	) -> Result<Vec<Option<RuleLookup>>> {
		let mut lookup_builders = Vec::with_capacity(lookups.len());
		for lookuplist in lookups {
			match lookuplist {
				Some(list) => {
					let mut builders = Vec::with_capacity(list.len());
					for l in list {
						match self.named_lookups_.get(&l.name) {
							Some(Some(b)) => builders.push(b.clone()),
							// LENIENT(L34): fontTools puts None in the list for an empty
							// named lookup and fails when the lookup is built
							// (AttributeError); this fails here.
							_ => {
								return Err(Error::feature_lib(
									"'NoneType' object has no attribute 'lookup_index'",
									location.clone(),
								));
							}
						}
					}
					lookup_builders.push(Some(RuleLookup::Many(builders)));
				}
				None => lookup_builders.push(None),
			}
		}
		Ok(lookup_builders)
	}

	/// An `Attach` statement (`add_attach_points`).
	pub fn add_attach_points(
		&mut self,
		_location: &Location,
		glyphs: &[String],
		contour_points: &IndexSet<i64>,
	) {
		for glyph in glyphs {
			self.attach_points_
				.entry(glyph.clone())
				.or_default()
				.extend(contour_points.iter().copied());
		}
	}

	/// A feature reference in `aalt` (`add_feature_reference`).
	pub fn add_feature_reference(&mut self, location: &Location, feature_name: &str) -> Result<()> {
		if self.cur_feature_name_.as_deref() != Some("aalt") {
			return Err(Error::feature_lib(
				"Feature references are only allowed inside \"feature aalt\"",
				location.clone(),
			));
		}
		self.aalt_features_
			.push((location.clone(), feature_name.to_owned()));
		Ok(())
	}

	/// `add_featureName`.
	pub fn add_feature_name(&mut self, tag: &str) {
		self.feature_names_.insert(tag.to_owned());
	}

	/// `add_cv_parameter`.
	pub fn add_cv_parameter(&mut self, tag: &str) {
		self.cv_parameters_.insert(tag.to_owned());
	}

	/// Adds new items to `cv_num_named_params_` or increments the count of
	/// existing items (`add_to_cv_num_named_params`).
	pub fn add_to_cv_num_named_params(&mut self, tag: &str) {
		*self.cv_num_named_params_.entry(tag.to_owned()).or_insert(0) += 1;
	}

	/// `add_cv_character`.
	pub fn add_cv_character(&mut self, character: i64, tag: &str) {
		self.cv_characters_
			.entry(tag.to_owned())
			.or_default()
			.push(character);
	}

	/// `set_base_axis`.
	pub fn set_base_axis(
		&mut self,
		bases: Vec<String>,
		scripts: Vec<BaseScriptRecord>,
		vertical: bool,
		minmax: Vec<BaseMinMax>,
	) {
		if vertical {
			self.base_vert_axis_ = Some((bases, scripts, minmax));
		} else {
			self.base_horiz_axis_ = Some((bases, scripts, minmax));
		}
	}

	/// A `parameters` statement of the `size` feature (`set_size_parameters`).
	pub fn set_size_parameters(
		&mut self,
		location: &Location,
		design_size: f64,
		subfamily_id: i64,
		range_start: f64,
		range_end: f64,
	) -> Result<()> {
		if self.cur_feature_name_.as_deref() != Some("size") {
			return Err(Error::feature_lib(
				format!(
					"Parameters statements are not allowed within \"feature {}\"",
					self.cur_feature_name_.as_deref().unwrap_or("None")
				),
				location.clone(),
			));
		}
		self.size_parameters_ = Some((design_size, subfamily_id, range_start, range_end));
		for (script, lang) in &self.language_systems {
			let key = (script.clone(), lang.clone(), "size".to_owned());
			self.features_.entry(key).or_default();
		}
		Ok(())
	}

	// GSUB rules

	/// `add_any_subst_`.
	fn add_any_subst_(
		&mut self,
		location: &Location,
		mapping: IndexMap<Vec<String>, Vec<String>>,
	) -> Result<()> {
		let lookup = self.get_lookup_(location, LookupKind::AnySubst, Some(&mapping))?;
		let mut lookup = lookup.borrow_mut();
		let LookupBuilder::AnySubst(lookup) = &mut *lookup else {
			return Err(Error::Assertion("AnySubstBuilder".to_owned()));
		};
		for (key, value) in mapping {
			let key = MapKey::Key(key);
			if let Some(existing) = lookup.mapping.get(&key) {
				let MapKey::Key(k) = &key else { continue };
				if *existing == value {
					log::info!(
						"Removing duplicate substitution from \"{}\" to \"{}\" at {}",
						k.join(", "),
						value.join(", "),
						location_string(location)
					);
				} else {
					return Err(Error::feature_lib(
						format!("Already defined substitution for \"{}\"", k.join(", ")),
						location.clone(),
					));
				}
			}
			lookup.mapping.insert(key, value);
		}
		Ok(())
	}

	/// GSUB 1 (`add_single_subst`).
	pub fn add_single_subst(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		suffix: &[Vec<String>],
		mapping: &IndexMap<String, String>,
		force_chain: bool,
	) -> Result<()> {
		if self.cur_feature_name_.as_deref() == Some("aalt") {
			for (from_glyph, to_glyph) in mapping {
				let alts = self.aalt_alternates_.entry(from_glyph.clone()).or_default();
				if !alts.contains(to_glyph) {
					alts.push(to_glyph.clone());
				}
			}
			return Ok(());
		}
		if !prefix.is_empty() || !suffix.is_empty() || force_chain {
			return self.add_single_subst_chained_(location, prefix, suffix, mapping);
		}
		self.add_any_subst_(
			location,
			mapping
				.iter()
				.map(|(k, v)| (vec![k.clone()], vec![v.clone()]))
				.collect(),
		)
	}

	/// GSUB 2 (`add_multiple_subst`).
	pub fn add_multiple_subst(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		glyph: &str,
		suffix: &[Vec<String>],
		replacements: &[String],
		force_chain: bool,
	) -> Result<()> {
		if !prefix.is_empty() || !suffix.is_empty() || force_chain {
			return self.add_multi_subst_chained_(location, prefix, glyph, suffix, replacements);
		}
		let mut mapping = IndexMap::new();
		mapping.insert(vec![glyph.to_owned()], replacements.to_vec());
		self.add_any_subst_(location, mapping)
	}

	/// GSUB 3 (`add_alternate_subst`).
	pub fn add_alternate_subst(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		glyph: &str,
		suffix: &[Vec<String>],
		replacement: &[String],
	) -> Result<()> {
		if self.cur_feature_name_.as_deref() == Some("aalt") {
			let alts = self.aalt_alternates_.entry(glyph.to_owned()).or_default();
			for g in replacement {
				if !alts.contains(g) {
					alts.push(g.clone());
				}
			}
			return Ok(());
		}
		let lookup = if !prefix.is_empty() || !suffix.is_empty() {
			let chain = self.get_lookup_(location, LookupKind::ChainContextSubst, None)?;
			let found = match &*chain.borrow() {
				LookupBuilder::ChainContextSubst(c) => c.find_chainable_alternate_subst(glyph),
				_ => None,
			};
			let lookup = match found {
				Some(l) => l,
				None => self.get_chained_lookup_(location, LookupKind::AlternateSubst),
			};
			self.add_contextual_rule(
				&chain,
				prefix.to_vec(),
				vec![vec![glyph.to_owned()]],
				suffix.to_vec(),
				vec![Some(RuleLookup::One(lookup.clone()))],
			)?;
			lookup
		} else {
			self.get_lookup_(location, LookupKind::AlternateSubst, None)?
		};
		let mut lookup = lookup.borrow_mut();
		let LookupBuilder::AlternateSubst(lookup) = &mut *lookup else {
			return Err(Error::Assertion("AlternateSubstBuilder".to_owned()));
		};
		let key = MapKey::Key(glyph.to_owned());
		if lookup.alternates.contains_key(&key) {
			return Err(Error::feature_lib(
				format!("Already defined alternates for glyph \"{glyph}\""),
				location.clone(),
			));
		}
		// We allow empty replacement glyphs here.
		lookup.alternates.insert(key, replacement.to_vec());
		Ok(())
	}

	/// GSUB 4 (`add_ligature_subst`).
	pub fn add_ligature_subst(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		glyphs: &[Vec<String>],
		suffix: &[Vec<String>],
		replacement: &str,
		force_chain: bool,
	) -> Result<()> {
		if !prefix.is_empty() || !suffix.is_empty() || force_chain {
			return self.add_ligature_subst_chained_(location, prefix, glyphs, suffix, replacement);
		}
		if glyphs.iter().any(Vec::is_empty) {
			return Err(Error::feature_lib(
				"Empty glyph class in substitution",
				location.clone(),
			));
		}
		// OpenType feature file syntax, section 5.d, "Ligature substitution":
		// "Since the OpenType specification does not allow ligature
		// substitutions to be specified on target sequences that contain glyph
		// classes, the implementation software will enumerate all specific
		// glyph sequences if glyph classes are detected"
		let mapping = fontoxide_otllib::builder::product(glyphs)
			.into_iter()
			.map(|g| (g, vec![replacement.to_owned()]))
			.collect();
		self.add_any_subst_(location, mapping)
	}

	/// GSUB 5/6 (`add_chain_context_subst`).
	pub fn add_chain_context_subst(
		&mut self,
		location: &Location,
		prefix: Vec<Vec<String>>,
		glyphs: Vec<Vec<String>>,
		suffix: Vec<Vec<String>>,
		lookups: &ChainLookups,
	) -> Result<()> {
		if glyphs.iter().any(Vec::is_empty)
			|| prefix.iter().any(Vec::is_empty)
			|| suffix.iter().any(Vec::is_empty)
		{
			return Err(Error::feature_lib(
				"Empty glyph class in contextual substitution",
				location.clone(),
			));
		}
		let lookup = self.get_lookup_(location, LookupKind::ChainContextSubst, None)?;
		let resolved = self.find_lookup_builders_(location, lookups)?;
		self.add_contextual_rule(&lookup, prefix, glyphs, suffix, resolved)
	}

	/// `add_single_subst_chained_`.
	fn add_single_subst_chained_(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		suffix: &[Vec<String>],
		mapping: &IndexMap<String, String>,
	) -> Result<()> {
		if mapping.is_empty()
			|| prefix.iter().any(Vec::is_empty)
			|| suffix.iter().any(Vec::is_empty)
		{
			return Err(Error::feature_lib(
				"Empty glyph class in contextual substitution",
				location.clone(),
			));
		}
		// https://github.com/fonttools/fonttools/issues/512
		// https://github.com/fonttools/fonttools/issues/2150
		let chain = self.get_lookup_(location, LookupKind::ChainContextSubst, None)?;
		let found = match &*chain.borrow() {
			LookupBuilder::ChainContextSubst(c) => c.find_chainable_single_subst(mapping),
			_ => None,
		};
		let sub = match found {
			Some(s) => s,
			None => self.get_chained_lookup_(location, LookupKind::SingleSubst),
		};
		if let LookupBuilder::SingleSubst(s) = &mut *sub.borrow_mut() {
			for (k, v) in mapping {
				s.mapping.insert(MapKey::Key(k.clone()), v.clone());
			}
		}
		let keys: Vec<String> = mapping.keys().cloned().collect();
		self.add_contextual_rule(
			&chain,
			prefix.to_vec(),
			vec![keys],
			suffix.to_vec(),
			vec![Some(RuleLookup::One(sub))],
		)
	}

	/// `add_multi_subst_chained_`.
	fn add_multi_subst_chained_(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		glyph: &str,
		suffix: &[Vec<String>],
		replacements: &[String],
	) -> Result<()> {
		if prefix.iter().any(Vec::is_empty) || suffix.iter().any(Vec::is_empty) {
			return Err(Error::feature_lib(
				"Empty glyph class in contextual substitution",
				location.clone(),
			));
		}
		// https://github.com/fonttools/fonttools/issues/3551
		let chain = self.get_lookup_(location, LookupKind::ChainContextSubst, None)?;
		let mut mapping = IndexMap::new();
		mapping.insert(glyph.to_owned(), replacements.to_vec());
		let found = match &*chain.borrow() {
			LookupBuilder::ChainContextSubst(c) => c.find_chainable_multiple_subst(&mapping),
			_ => None,
		};
		let sub = match found {
			Some(s) => s,
			None => self.get_chained_lookup_(location, LookupKind::MultipleSubst),
		};
		if let LookupBuilder::MultipleSubst(s) = &mut *sub.borrow_mut() {
			s.mapping
				.insert(MapKey::Key(glyph.to_owned()), replacements.to_vec());
		}
		// https://github.com/fonttools/fonttools/issues/4016
		self.add_contextual_rule(
			&chain,
			prefix.to_vec(),
			vec![vec![glyph.to_owned()]],
			suffix.to_vec(),
			vec![Some(RuleLookup::One(sub))],
		)
	}

	/// `add_ligature_subst_chained_`.
	fn add_ligature_subst_chained_(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		glyphs: &[Vec<String>],
		suffix: &[Vec<String>],
		replacement: &str,
	) -> Result<()> {
		// https://github.com/fonttools/fonttools/issues/3701
		if prefix.iter().any(Vec::is_empty) || suffix.iter().any(Vec::is_empty) {
			return Err(Error::feature_lib(
				"Empty glyph class in contextual substitution",
				location.clone(),
			));
		}
		let chain = self.get_lookup_(location, LookupKind::ChainContextSubst, None)?;
		let found = match &*chain.borrow() {
			LookupBuilder::ChainContextSubst(c) => {
				c.find_chainable_ligature_subst(glyphs, replacement)
			}
			_ => None,
		};
		let sub = match found {
			Some(s) => s,
			None => self.get_chained_lookup_(location, LookupKind::LigatureSubst),
		};
		if let LookupBuilder::LigatureSubst(s) = &mut *sub.borrow_mut() {
			for g in fontoxide_otllib::builder::product(glyphs) {
				let key = MapKey::Key(g);
				let existing = s.ligatures.get(&key).map_or(replacement, String::as_str);
				if existing != replacement {
					let MapKey::Key(g) = &key else { continue };
					return Err(Error::feature_lib(
						format!(
							"Conflicting ligature sub rules: '{}' maps to '{existing}' and '{replacement}'",
							tuple_repr(g)
						),
						location.clone(),
					));
				}
				s.ligatures.insert(key, replacement.to_owned());
			}
		}
		let mut chain = chain.borrow_mut();
		if let LookupBuilder::ChainContextSubst(c) = &mut *chain {
			c.rules.push(ChainContextualRule::new(
				prefix.to_vec(),
				glyphs.to_vec(),
				suffix.to_vec(),
				vec![Some(RuleLookup::One(sub))],
			));
		}
		Ok(())
	}

	/// GSUB 8 (`add_reverse_chain_single_subst`).
	pub fn add_reverse_chain_single_subst(
		&mut self,
		location: &Location,
		old_prefix: Vec<Vec<String>>,
		old_suffix: Vec<Vec<String>>,
		mapping: IndexMap<String, String>,
	) -> Result<()> {
		if mapping.is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in substitution",
				location.clone(),
			));
		}
		let lookup = self.get_lookup_(location, LookupKind::ReverseChainSingleSubst, None)?;
		if let LookupBuilder::ReverseChainSingleSubst(l) = &mut *lookup.borrow_mut() {
			l.rules.push((old_prefix, old_suffix, mapping));
		}
		Ok(())
	}

	// GPOS rules

	/// GPOS 1 (`add_single_pos`).
	pub fn add_single_pos(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		suffix: &[Vec<String>],
		pos: &[(Vec<String>, Option<ValueRecord>)],
		force_chain: bool,
	) -> Result<()> {
		if !prefix.is_empty() || !suffix.is_empty() || force_chain {
			return self.add_single_pos_chained_(location, prefix, suffix, pos);
		}
		let lookup = self.get_lookup_(location, LookupKind::SinglePos, None)?;
		for (glyphs, value) in pos {
			if glyphs.is_empty() {
				return Err(Error::feature_lib(
					"Empty glyph class in positioning rule",
					location.clone(),
				));
			}
			let ot_value_record =
				self.make_open_type_value_record(location, value.as_ref(), false)?;
			let mut lookup = lookup.borrow_mut();
			let LookupBuilder::SinglePos(lookup) = &mut *lookup else {
				return Err(Error::Assertion("SinglePosBuilder".to_owned()));
			};
			for glyph in glyphs {
				lookup
					.add_pos(&otl_location(location), glyph, ot_value_record.clone())
					.map_err(wrap_otl_error)?;
			}
		}
		Ok(())
	}

	/// GPOS 2 with glyph classes (`add_class_pair_pos`).
	pub fn add_class_pair_pos(
		&mut self,
		location: &Location,
		glyphclass1: &[String],
		value1: Option<&ValueRecord>,
		glyphclass2: &[String],
		value2: Option<&ValueRecord>,
	) -> Result<()> {
		if glyphclass1.is_empty() || glyphclass2.is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in positioning rule",
				location.clone(),
			));
		}
		let lookup = self.get_lookup_(location, LookupKind::PairPos, None)?;
		let v1 = self.make_open_type_value_record(location, value1, true)?;
		let v2 = self.make_open_type_value_record(location, value2, true)?;
		let cls1 = glyph_set_key(glyphclass1);
		let cls2 = glyph_set_key(glyphclass2);
		if let LookupBuilder::PairPos(l) = &mut *lookup.borrow_mut() {
			l.add_class_pair(&otl_location(location), cls1, v1, cls2, v2);
		}
		Ok(())
	}

	/// GPOS 2 with glyphs (`add_specific_pair_pos`).
	pub fn add_specific_pair_pos(
		&mut self,
		location: &Location,
		glyph1: &str,
		value1: Option<&ValueRecord>,
		glyph2: &str,
		value2: Option<&ValueRecord>,
	) -> Result<()> {
		if glyph1.is_empty() || glyph2.is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in positioning rule",
				location.clone(),
			));
		}
		let lookup = self.get_lookup_(location, LookupKind::PairPos, None)?;
		let v1 = self.make_open_type_value_record(location, value1, true)?;
		let v2 = self.make_open_type_value_record(location, value2, true)?;
		if let LookupBuilder::PairPos(l) = &mut *lookup.borrow_mut() {
			l.add_glyph_pair(&otl_location(location), glyph1, v1, glyph2, v2);
		}
		Ok(())
	}

	/// GPOS 3 (`add_cursive_pos`).
	pub fn add_cursive_pos(
		&mut self,
		location: &Location,
		glyphclass: &[String],
		entry_anchor: Option<&Anchor>,
		exit_anchor: Option<&Anchor>,
	) -> Result<()> {
		if glyphclass.is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in positioning rule",
				location.clone(),
			));
		}
		let lookup = self.get_lookup_(location, LookupKind::CursivePos, None)?;
		let entry = self.make_open_type_anchor(location, entry_anchor)?;
		let exit = self.make_open_type_anchor(location, exit_anchor)?;
		if let LookupBuilder::CursivePos(l) = &mut *lookup.borrow_mut() {
			l.add_attachment(&otl_location(location), glyphclass, entry, exit);
		}
		Ok(())
	}

	/// GPOS 4 (`add_mark_base_pos`).
	pub fn add_mark_base_pos(
		&mut self,
		location: &Location,
		bases: &[String],
		marks: &AnchorMarks,
	) -> Result<()> {
		let builder = self.get_lookup_(location, LookupKind::MarkBasePos, None)?;
		self.add_marks_(location, &builder, marks)?;
		if bases.is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in positioning rule",
				location.clone(),
			));
		}
		for (base_anchor, mark_class) in marks {
			let ot_base_anchor = self.make_open_type_anchor(location, base_anchor.as_ref())?;
			let name = mark_class.borrow().name.clone();
			if let LookupBuilder::MarkBasePos(b) = &mut *builder.borrow_mut() {
				for base in bases {
					set_anchor(
						b.bases.entry(base.clone()).or_default(),
						&name,
						&ot_base_anchor,
					);
				}
			}
		}
		Ok(())
	}

	/// GPOS 5 (`add_mark_lig_pos`).
	pub fn add_mark_lig_pos(
		&mut self,
		location: &Location,
		ligatures: &[String],
		components: &[AnchorMarks],
	) -> Result<()> {
		let builder = self.get_lookup_(location, LookupKind::MarkLigPos, None)?;
		let mut component_anchors = Vec::with_capacity(components.len());
		if ligatures.is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in positioning rule",
				location.clone(),
			));
		}
		for marks in components {
			let mut anchors = IndexMap::new();
			self.add_marks_(location, &builder, marks)?;
			for (lig_anchor, mark_class) in marks {
				let anchor = self.make_open_type_anchor(location, lig_anchor.as_ref())?;
				set_anchor(&mut anchors, &mark_class.borrow().name, &anchor);
			}
			component_anchors.push(anchors);
		}
		if let LookupBuilder::MarkLigPos(b) = &mut *builder.borrow_mut() {
			for glyph in ligatures {
				b.ligatures.insert(glyph.clone(), component_anchors.clone());
			}
		}
		Ok(())
	}

	/// GPOS 6 (`add_mark_mark_pos`).
	pub fn add_mark_mark_pos(
		&mut self,
		location: &Location,
		base_marks: &[String],
		marks: &AnchorMarks,
	) -> Result<()> {
		let builder = self.get_lookup_(location, LookupKind::MarkMarkPos, None)?;
		self.add_marks_(location, &builder, marks)?;
		if base_marks.is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in positioning rule",
				location.clone(),
			));
		}
		for (base_anchor, mark_class) in marks {
			let ot_base_anchor = self.make_open_type_anchor(location, base_anchor.as_ref())?;
			let name = mark_class.borrow().name.clone();
			if let LookupBuilder::MarkMarkPos(b) = &mut *builder.borrow_mut() {
				for base_mark in base_marks {
					set_anchor(
						b.base_marks.entry(base_mark.clone()).or_default(),
						&name,
						&ot_base_anchor,
					);
				}
			}
		}
		Ok(())
	}

	/// GPOS 7/8 (`add_chain_context_pos`).
	pub fn add_chain_context_pos(
		&mut self,
		location: &Location,
		prefix: Vec<Vec<String>>,
		glyphs: Vec<Vec<String>>,
		suffix: Vec<Vec<String>>,
		lookups: &ChainLookups,
	) -> Result<()> {
		if glyphs.iter().any(Vec::is_empty)
			|| prefix.iter().any(Vec::is_empty)
			|| suffix.iter().any(Vec::is_empty)
		{
			return Err(Error::feature_lib(
				"Empty glyph class in contextual positioning rule",
				location.clone(),
			));
		}
		let lookup = self.get_lookup_(location, LookupKind::ChainContextPos, None)?;
		let resolved = self.find_lookup_builders_(location, lookups)?;
		self.add_contextual_rule(&lookup, prefix, glyphs, suffix, resolved)
	}

	/// `add_single_pos_chained_`.
	fn add_single_pos_chained_(
		&mut self,
		location: &Location,
		prefix: &[Vec<String>],
		suffix: &[Vec<String>],
		pos: &[(Vec<String>, Option<ValueRecord>)],
	) -> Result<()> {
		if pos.is_empty() || prefix.iter().any(Vec::is_empty) || suffix.iter().any(Vec::is_empty) {
			return Err(Error::feature_lib(
				"Empty glyph class in contextual positioning rule",
				location.clone(),
			));
		}
		// https://github.com/fonttools/fonttools/issues/514
		let chain = self.get_lookup_(location, LookupKind::ChainContextPos, None)?;
		let mut targets: Vec<ChainTarget> = match &*chain.borrow() {
			LookupBuilder::ChainContextPos(c) => c.targets(),
			_ => Vec::new(),
		};
		let mut subs: Vec<Option<RuleLookup>> = Vec::with_capacity(pos.len());
		for (glyphs, value) in pos {
			let Some(value) = value else {
				subs.push(None);
				continue;
			};
			let ot_value = self.make_open_type_value_record(location, Some(value), false)?;
			let ot_value_or_empty = ot_value.clone().unwrap_or_default();
			let found = match &*chain.borrow() {
				LookupBuilder::ChainContextPos(c) => {
					c.find_chainable_single_pos(&targets, glyphs, &ot_value_or_empty)
				}
				_ => None,
			};
			let sub = match found {
				Some(s) => s,
				None => {
					let sub = self.get_chained_lookup_(location, LookupKind::SinglePos);
					targets.push(ChainTarget::Lookup(Some(RuleLookup::One(sub.clone()))));
					sub
				}
			};
			if let LookupBuilder::SinglePos(s) = &mut *sub.borrow_mut() {
				for glyph in glyphs {
					s.add_pos(&otl_location(location), glyph, ot_value.clone())
						.map_err(Error::OtlLib)?;
				}
			}
			subs.push(Some(RuleLookup::One(sub)));
		}
		let glyphs: Vec<Vec<String>> = pos.iter().map(|(g, _)| g.clone()).collect();
		self.add_contextual_rule(&chain, prefix.to_vec(), glyphs, suffix.to_vec(), subs)
	}

	/// Adds the marks of mark classes to a mark attachment lookup, checking that
	/// no class is used twice in the rule (`add_marks_`).
	fn add_marks_(
		&mut self,
		location: &Location,
		lookup: &LookupRef,
		marks: &AnchorMarks,
	) -> Result<()> {
		let mut seen_mark_classes = HashSet::new();
		for (_, mark_class) in marks {
			let name = mark_class.borrow().name.clone();
			if !seen_mark_classes.insert(name.clone()) {
				return Err(Error::feature_lib(
					format!("Mark class @{name} used more than once in the same anchor list"),
					location.clone(),
				));
			}
		}
		for (_, mark_class) in marks {
			let (name, definitions) = {
				let mc = mark_class.borrow();
				(mc.name.clone(), mc.definitions.clone())
			};
			for mark_class_def in definitions {
				for mark in mark_class_def.glyphs.glyph_set().iter() {
					let existing = marks_of(&lookup.borrow())?.get(mark).map(|m| m.0.clone());
					match existing {
						None => {
							let ot_mark_anchor = self
								.make_open_type_anchor(location, Some(&mark_class_def.anchor))?
								.ok_or_else(|| Error::Value("no anchor".to_owned()))?;
							marks_of_mut(&mut lookup.borrow_mut())?
								.insert(mark.clone(), (name.clone(), ot_mark_anchor));
						}
						Some(existing_mark_class) => {
							if name != existing_mark_class {
								return Err(Error::feature_lib(
									format!(
										"Glyph {mark} cannot be in both @{existing_mark_class} and @{name}"
									),
									location.clone(),
								));
							}
						}
					}
				}
			}
		}
		Ok(())
	}

	/// A `subtable` statement (`add_subtable_break`).
	pub fn add_subtable_break(&mut self, location: &Location) -> Result<()> {
		match &self.cur_lookup_ {
			Some(lookup) => {
				lookup
					.borrow_mut()
					.add_subtable_break(&otl_location(location));
				Ok(())
			}
			None => Err(Error::Value(
				"'NoneType' object has no attribute 'add_subtable_break'".to_owned(),
			)),
		}
	}

	/// `setGlyphClass_`.
	fn set_glyph_class_(
		&mut self,
		location: &Location,
		glyph: &str,
		glyph_class: u16,
	) -> Result<()> {
		if let Some((old_class, old_location)) = self.glyph_class_defs_.get(glyph)
			&& *old_class != glyph_class
		{
			return Err(Error::feature_lib(
				format!(
					"Glyph {glyph} was assigned to a different class at {}",
					location_string(old_location)
				),
				location.clone(),
			));
		}
		self.glyph_class_defs_
			.insert(glyph.to_owned(), (glyph_class, location.clone()));
		Ok(())
	}

	/// A `GlyphClassDef` statement (`add_glyphClassDef`).
	pub fn add_glyph_class_def(
		&mut self,
		location: &Location,
		base_glyphs: &[String],
		ligature_glyphs: &[String],
		mark_glyphs: &[String],
		component_glyphs: &[String],
	) -> Result<()> {
		for glyph in base_glyphs {
			self.set_glyph_class_(location, glyph, 1)?;
		}
		for glyph in ligature_glyphs {
			self.set_glyph_class_(location, glyph, 2)?;
		}
		for glyph in mark_glyphs {
			self.set_glyph_class_(location, glyph, 3)?;
		}
		for glyph in component_glyphs {
			self.set_glyph_class_(location, glyph, 4)?;
		}
		Ok(())
	}

	/// A `LigatureCaretByIndex` statement (`add_ligatureCaretByIndex_`).
	pub fn add_ligature_caret_by_index_(
		&mut self,
		_location: &Location,
		glyphs: &[String],
		carets: &[i64],
	) {
		for glyph in glyphs {
			if !self.lig_caret_points_.contains_key(glyph) {
				self.lig_caret_points_
					.insert(glyph.clone(), carets.to_vec());
			}
		}
	}

	/// A ligature caret position, with a device table if it varies
	/// (`makeLigCaret`).
	fn make_lig_caret(&mut self, location: &Location, caret: &ScalarValue) -> Result<CaretCoord> {
		match caret {
			ScalarValue::Int(i) => Ok(CaretCoord::Coord(to_i16(*i)?)),
			ScalarValue::Variable(v) => {
				let (default, device) = self.make_variable_pos(location, v)?;
				Ok(match device {
					Some(device) => CaretCoord::WithDevice(to_i16(default)?, Some(device)),
					None => CaretCoord::Coord(to_i16(default)?),
				})
			}
		}
	}

	/// A `LigatureCaretByPos` statement (`add_ligatureCaretByPos_`).
	pub fn add_ligature_caret_by_pos_(
		&mut self,
		location: &Location,
		glyphs: &[String],
		carets: &[ScalarValue],
	) -> Result<()> {
		let carets = carets
			.iter()
			.map(|c| self.make_lig_caret(location, c))
			.collect::<Result<Vec<_>>>()?;
		for glyph in glyphs {
			if !self.lig_caret_coords_.contains_key(glyph) {
				self.lig_caret_coords_.insert(glyph.clone(), carets.clone());
			}
		}
		Ok(())
	}

	/// `add_os2_field`.
	pub fn add_os2_field(&mut self, key: &str, value: OS2Value) {
		self.os2_.insert(key.to_owned(), value);
	}

	/// `add_hhea_field`.
	pub fn add_hhea_field(&mut self, key: &str, value: i64) {
		self.hhea_.insert(key.to_owned(), value);
	}

	/// `add_vhea_field`.
	pub fn add_vhea_field(&mut self, key: &str, value: i64) {
		self.vhea_.insert(key.to_owned(), value);
	}

	/// A `conditionset` statement (`add_conditionset`): the ranges are
	/// normalized, with the `avar` mappings.
	pub fn add_conditionset(
		&mut self,
		location: &Location,
		key: &str,
		value: &IndexMap<String, (IntOrFloat, IntOrFloat)>,
	) -> Result<()> {
		if !self.font.contains(Tag::new(b"fvar")) {
			return Err(Error::feature_lib(
				"Cannot add feature variations to a font without an 'fvar' table",
				location.clone(),
			));
		}
		if self.conditionsets_.contains_key(key) {
			return Err(Error::feature_lib(
				format!("Condition set '{key}' has the same name as a previous condition set"),
				location.clone(),
			));
		}
		// Normalize
		let mut normalized: AxisBox = IndexMap::new();
		for (tag, (bottom, top)) in value {
			let triple = self
				.axes
				.iter()
				.find(|(t, _)| t == tag)
				.map(|(_, triple)| *triple)
				.ok_or_else(|| Error::Value(format!("KeyError: {}", str_repr(tag))))?;
			normalized.insert(
				tag.clone(),
				(
					normalize_value(bottom.as_f64(), triple, false)?,
					normalize_value(top.as_f64(), triple, false)?,
				),
			);
		}
		// NOTE: This might result in rounding errors (off-by-ones) compared to
		// rules in Designspace files, since we're working with what's in the
		// `avar` table rather than the original values.
		if let Some(avar) = self.font.get::<Avar>()? {
			for (axis, range) in normalized.iter_mut() {
				if let Some(mapping) =
					Tag::from_str_padded(axis).and_then(|t| avar.segments.get(&t))
				{
					*range = (
						piecewise_linear_map(range.0, mapping),
						piecewise_linear_map(range.1, mapping),
					);
				}
			}
		}
		self.conditionsets_.insert(key.to_owned(), normalized);
		Ok(())
	}

	/// The default value of a variable scalar, and a device table with its
	/// variation index if it varies (`makeVariablePos`).
	fn make_variable_pos(
		&mut self,
		location: &Location,
		varscalar: &VariableScalar,
	) -> Result<(i64, Option<ot::Device>)> {
		let (Some(store), Some(scalars)) =
			(self.varstorebuilder.as_mut(), self.scalar_builder.as_mut())
		else {
			return Err(Error::feature_lib(
				"Can't define a variable scalar in a non-variable font",
				location.clone(),
			));
		};
		if !varscalar.does_vary() {
			return Ok((scalars.default_value(varscalar)?, None));
		}
		let (default, index) = match scalars.add_to_variation_store(varscalar, store) {
			Ok(r) => r,
			// A `VarLibError`.
			Err(Error::TtLib(fontoxide_ttlib::Error::TtLib(_))) => {
				return Err(Error::feature_lib(
					"Failed to compute deltas for variable scalar",
					location.clone(),
				));
			}
			Err(e) => return Err(e),
		};
		let device = (index != 0xFFFF_FFFF).then(|| build_var_dev_table(index as u32));
		Ok((default, device))
	}

	/// A coordinate of an anchor and its device table (`makeAnchorPos`).
	fn make_anchor_pos(
		&mut self,
		varscalar: &ScalarValue,
		device_table: Option<&Device>,
		location: &Location,
	) -> Result<(i64, Option<ot::Device>)> {
		match varscalar {
			ScalarValue::Int(v) => {
				let device = match device_table {
					Some(d) => build_device(&device_dict(d))?,
					None => None,
				};
				Ok((*v, device))
			}
			ScalarValue::Variable(v) => {
				let (default, device) = self.make_variable_pos(location, v)?;
				if device.is_some() && device_table.is_some() {
					return Err(Error::feature_lib(
						"Can't define a device coordinate and variable scalar",
						location.clone(),
					));
				}
				Ok((default, device))
			}
		}
	}

	/// `ast.Anchor` to `otTables.Anchor` (`makeOpenTypeAnchor`).
	fn make_open_type_anchor(
		&mut self,
		location: &Location,
		anchor: Option<&Anchor>,
	) -> Result<Option<ot::Anchor>> {
		let Some(anchor) = anchor else {
			return Ok(None);
		};
		let (x, device_x) =
			self.make_anchor_pos(&anchor.x, anchor.x_device_table.as_ref(), location)?;
		let (y, device_y) =
			self.make_anchor_pos(&anchor.y, anchor.y_device_table.as_ref(), location)?;
		let point = anchor.contourpoint.map(to_u16).transpose()?;
		Ok(Some(build_anchor(
			to_i16(x)?,
			to_i16(y)?,
			point,
			device_x,
			device_y,
		)?))
	}

	/// `ast.ValueRecord` to `otBase.ValueRecord` (`makeOpenTypeValueRecord`).
	fn make_open_type_value_record(
		&mut self,
		location: &Location,
		v: Option<&ValueRecord>,
		pair_pos_context: bool,
	) -> Result<Option<OtValueRecord>> {
		let Some(v) = v.filter(|v| v.is_truthy()) else {
			return Ok(None);
		};
		let mut vr = OtValueRecord::default();
		let mut any = false;
		let scalars = [
			(&v.x_placement, &v.x_pla_device),
			(&v.y_placement, &v.y_pla_device),
			(&v.x_advance, &v.x_adv_device),
			(&v.y_advance, &v.y_adv_device),
		];
		for (i, (value, fea_device)) in scalars.into_iter().enumerate() {
			let (value, device) = match value {
				None | Some(ScalarValue::Int(0)) => continue,
				Some(ScalarValue::Int(v)) => (*v, None),
				Some(ScalarValue::Variable(scalar)) => {
					if fea_device.as_ref().is_some_and(|d| !d.is_empty()) {
						return Err(Error::feature_lib(
							"Can't define a device coordinate and variable scalar",
							location.clone(),
						));
					}
					self.make_variable_pos(location, scalar)?
				}
			};
			let value = Some(to_i16(value)?);
			let device = device.map(|d| Some(Box::new(d)));
			any = true;
			match i {
				0 => {
					vr.x_placement = value;
					if device.is_some() {
						vr.x_pla_device = device;
					}
				}
				1 => {
					vr.y_placement = value;
					if device.is_some() {
						vr.y_pla_device = device;
					}
				}
				2 => {
					vr.x_advance = value;
					if device.is_some() {
						vr.x_adv_device = device;
					}
				}
				_ => {
					vr.y_advance = value;
					if device.is_some() {
						vr.y_adv_device = device;
					}
				}
			}
		}
		let devices = [
			&v.x_pla_device,
			&v.y_pla_device,
			&v.x_adv_device,
			&v.y_adv_device,
		];
		for (i, device) in devices.into_iter().enumerate() {
			let Some(device) = device.as_ref().filter(|d| !d.is_empty()) else {
				continue;
			};
			let device = Some(build_device(&device_dict(device))?.map(Box::new));
			any = true;
			match i {
				0 => vr.x_pla_device = device,
				1 => vr.y_pla_device = device,
				2 => vr.x_adv_device = device,
				_ => vr.y_adv_device = device,
			}
		}
		if pair_pos_context && !any {
			if v.vertical {
				vr.y_advance = Some(0);
			} else {
				vr.x_advance = Some(0);
			}
		}
		Ok(Some(vr))
	}

	/// Adds the lookup debugging information to the `Debg` table (`buildDebg`).
	// LENIENT(L34): fontoxide does not implement the `Debg` table; it is added as raw
	// data, which is the JSON that fontTools compiles. An existing `Debg` table
	// is replaced rather than merged.
	fn build_debg(&mut self) {
		let mut json = String::from("{");
		json += &json_string(crate::lookup_debug_info::LOOKUP_DEBUG_INFO_KEY);
		json += ": {";
		let tables: Vec<String> = self
			.lookup_locations
			.iter()
			.map(|(tag, infos)| {
				let items: Vec<String> = infos
					.iter()
					.map(|(index, info)| {
						let feature = match &info.feature {
							Some(f) => {
								let items: Vec<String> = f.iter().map(|s| json_string(s)).collect();
								format!("[{}]", items.join(", "))
							}
							None => "null".to_owned(),
						};
						format!(
							"{}: [{}, {}, {feature}]",
							json_string(index),
							json_string(&info.location),
							info.name
								.as_deref()
								.map_or_else(|| "null".to_owned(), json_string),
						)
					})
					.collect();
				format!("{}: {{{}}}", json_string(tag), items.join(", "))
			})
			.collect();
		json += &tables.join(", ");
		json += "}}";
		self.font
			.insert(Box::new(fontoxide_ttlib::table::DefaultTable::new(
				Tag::new(b"Debg"),
				json.into_bytes(),
			)));
	}
}

/// `json.dumps(s)` of a string (with `ensure_ascii`).
fn json_string(s: &str) -> String {
	let mut out = String::from("\"");
	for c in s.chars() {
		match c {
			'"' => out += "\\\"",
			'\\' => out += "\\\\",
			'\n' => out += "\\n",
			'\r' => out += "\\r",
			'\t' => out += "\\t",
			'\u{8}' => out += "\\b",
			'\u{c}' => out += "\\f",
			c if (c as u32) < 0x20 || (c as u32) > 0x7E => {
				let mut buf = [0u16; 2];
				for unit in c.encode_utf16(&mut buf) {
					out += &format!("\\u{unit:04x}");
				}
			}
			c => out.push(c),
		}
	}
	out.push('"');
	out
}

/// `dict(deviceTable)`: the last delta of a size wins.
fn device_dict(device: &Device) -> BTreeMap<i64, i64> {
	device.iter().copied().collect()
}

/// `anchors[mark_class] = anchor`, where a `None` anchor (`<anchor NULL>`)
/// builds the same as no anchor.
fn set_anchor(
	anchors: &mut IndexMap<String, ot::Anchor>,
	mark_class: &str,
	anchor: &Option<ot::Anchor>,
) {
	match anchor {
		Some(a) => {
			anchors.insert(mark_class.to_owned(), a.clone());
		}
		None => {
			anchors.shift_remove(mark_class);
		}
	}
}

/// The marks of a mark attachment lookup builder (`lookupBuilder.marks`).
fn marks_of(builder: &LookupBuilder) -> Result<&fontoxide_otllib::builder::MarkMap> {
	match builder {
		LookupBuilder::MarkBasePos(b) => Ok(&b.marks),
		LookupBuilder::MarkLigPos(b) => Ok(&b.marks),
		LookupBuilder::MarkMarkPos(b) => Ok(&b.marks),
		_ => Err(Error::Value("AttributeError: 'marks'".to_owned())),
	}
}

/// [`marks_of`], mutably.
fn marks_of_mut(builder: &mut LookupBuilder) -> Result<&mut fontoxide_otllib::builder::MarkMap> {
	match builder {
		LookupBuilder::MarkBasePos(b) => Ok(&mut b.marks),
		LookupBuilder::MarkLigPos(b) => Ok(&mut b.marks),
		LookupBuilder::MarkMarkPos(b) => Ok(&mut b.marks),
		_ => Err(Error::Value("AttributeError: 'marks'".to_owned())),
	}
}

/// Adds a contextual rule, merging it with the last rule if possible
/// (`_add_contextual_rule`).
///
/// Consecutive rules that share the same prefix, suffix and lookups, and have a
/// single input position, can be merged into one rule with broader input
/// coverage. This produces more compact binary tables (often format 3).
impl Builder<'_> {
	fn add_contextual_rule(
		&mut self,
		chain_ref: &LookupRef,
		prefix: Vec<Vec<String>>,
		glyphs: Vec<Vec<String>>,
		suffix: Vec<Vec<String>>,
		lookups: Vec<Option<RuleLookup>>,
	) -> Result<()> {
		let mut chain = chain_ref.borrow_mut();
		let rules = match &mut *chain {
			LookupBuilder::ChainContextPos(c) | LookupBuilder::ChainContextSubst(c) => &mut c.rules,
			_ => return Err(Error::Assertion("ChainContextualBuilder".to_owned())),
		};
		let rule_count = rules.len();
		if glyphs.len() == 1
			&& let Some(ChainContextualRule::Rule {
				prefix: last_prefix,
				glyphs: last_glyphs,
				suffix: last_suffix,
				lookups: last_lookups,
			}) = rules.last_mut()
			&& last_glyphs.len() == 1
			&& *last_prefix == prefix
			&& *last_suffix == suffix
			&& *last_lookups == lookups
		{
			// `set(last.glyphs[0]).update(glyphs[0])`: the glyph order within
			// the class does not matter to the builders, which sort it. The set
			// of the class is kept between consecutive merges, so that merging
			// many rules takes linear time.
			let class = &mut last_glyphs[0];
			let cache = self.merge_cache_.take().filter(|c| {
				c.chain == Rc::as_ptr(chain_ref)
					&& c.rule_count == rule_count
					&& c.len == class.len()
			});
			let mut seen = match cache {
				Some(c) => c.set,
				None => {
					let mut seen: HashSet<String> = HashSet::with_capacity(class.len());
					class.retain(|g| seen.insert(g.clone()));
					seen
				}
			};
			for g in &glyphs[0] {
				if seen.insert(g.clone()) {
					class.push(g.clone());
				}
			}
			self.merge_cache_ = Some(MergeCache {
				chain: Rc::as_ptr(chain_ref),
				rule_count,
				len: class.len(),
				set: seen,
			});
			return Ok(());
		}
		rules.push(ChainContextualRule::new(prefix, glyphs, suffix, lookups));
		Ok(())
	}
}

/// The glyph set of the last rule that [`Builder::add_contextual_rule`]
/// merged into.
#[derive(Debug)]
struct MergeCache {
	chain: *const RefCell<LookupBuilder>,
	rule_count: usize,
	len: usize,
	set: HashSet<String>,
}
