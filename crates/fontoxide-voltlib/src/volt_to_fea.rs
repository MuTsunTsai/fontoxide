//! MS VOLT `.vtp` to AFDKO `.fea` OpenType Layout converter
//! (`fontTools.voltLib.voltToFea`).
//!
//! It is also possible to convert font files with a `TSIV` table (as saved from
//! VOLT); in this case the glyph names used in the VOLT project are mapped to
//! the actual glyph names in the font when written to the feature file.
//!
//! Limitations (as in fontTools):
//!
//! - Not all VOLT features are supported; the converter fails on what it does
//!   not understand.
//! - AFDKO feature file syntax for mark positioning is awkward and does not
//!   allow setting the mark coverage. It also defines mark anchors globally, so
//!   some mark positioning lookups might cover more marks than in the VOLT file.
//! - VOLT allows subtable breaks in any lookup type, but feature file
//!   implementations vary in their support.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use fontoxide_fealib::ast::{
	self, Anchor, AnchorMarks, ClassName, GlyphClass, GlyphClassItem, GlyphContainer, MarkClass,
	MarkClassRef, ScalarValue, ValueRecord, glyph_as_fea,
};
use fontoxide_ttlib::{Font, Tag};
use indexmap::{IndexMap, IndexSet};

use crate::ast::{
	AnchorDefinition, Coverage, CoverageItem, GlyphDefinition, GroupDefinition, LookupDefinition,
	Pos, PositionDefinition, ProcessMarks, ScriptDefinition, SettingDefinition, SettingValue,
	Statement as VStatement, SubstitutionKind, VoltFile,
};
use crate::error::{Error, Location, Result};
use crate::parser::{Parser, ParserInput};

/// The tables the converter can write (`TABLES`).
pub const TABLES: [&str; 3] = ["GDEF", "GSUB", "GPOS"];

/// A feaLib object as `VoltToFea` handles it in Python: a glyph name string,
/// a `GlyphName` (whose `glyph` may itself be an object), a `GlyphClass` or a
/// `GlyphClassName`.
#[derive(Debug, Clone)]
enum Fea {
	Str(String),
	GlyphName(Box<Fea>),
	GlyphClass(Vec<Fea>),
	ClassName(Rc<ClassDef>),
}

/// A glyph class made from a VOLT group: its items as Python has them, and the
/// feaLib definition.
#[derive(Debug)]
struct ClassDef {
	items: Vec<Fea>,
	fea: Rc<ast::GlyphClassDefinition>,
}

impl Fea {
	/// `glyphSet()`.
	fn glyph_set(&self) -> Result<Vec<Fea>> {
		match self {
			Fea::Str(_) => Err(Error::Python(
				"AttributeError: 'str' object has no attribute 'glyphSet'".to_owned(),
			)),
			Fea::GlyphName(g) => Ok(vec![(**g).clone()]),
			Fea::GlyphClass(items) => Ok(items.clone()),
			Fea::ClassName(c) => Ok(c.items.clone()),
		}
	}

	/// `asFea(x)`.
	fn as_fea(&self) -> String {
		match self {
			Fea::Str(s) => glyph_as_fea(s),
			Fea::GlyphName(g) => g.as_fea(),
			Fea::GlyphClass(items) => {
				let items: Vec<String> = items.iter().map(Fea::as_fea).collect();
				format!("[{}]", items.join(" "))
			}
			Fea::ClassName(c) => format!("@{}", c.fea.name),
		}
	}

	/// The glyph name, if this writes as a plain glyph name.
	fn plain_name(&self) -> Option<&str> {
		match self {
			Fea::Str(s) => Some(s),
			Fea::GlyphName(g) => g.plain_name(),
			_ => None,
		}
	}

	/// The class definition, if this writes as a class name.
	fn class_def(&self) -> Option<&Rc<ClassDef>> {
		match self {
			Fea::ClassName(c) => Some(c),
			Fea::GlyphName(g) => g.class_def(),
			_ => None,
		}
	}

	/// The feaLib glyph-containing object with the same `asFea()`.
	fn to_container(&self) -> GlyphContainer {
		if let Some(name) = self.plain_name() {
			return GlyphContainer::GlyphName(ast::GlyphName::new(name));
		}
		if let Some(c) = self.class_def() {
			return GlyphContainer::GlyphClassName(class_name(c));
		}
		match self {
			Fea::GlyphClass(items) => GlyphContainer::GlyphClass(glyph_class(items)),
			Fea::GlyphName(g) => g.to_container(),
			// Plain names and class names are handled above.
			Fea::Str(s) => GlyphContainer::GlyphName(ast::GlyphName::new(s.clone())),
			Fea::ClassName(c) => GlyphContainer::GlyphClassName(class_name(c)),
		}
	}
}

fn class_name(c: &ClassDef) -> ast::GlyphClassName {
	ast::GlyphClassName {
		glyphclass: c.fea.clone(),
		location: None,
	}
}

/// `GlyphClass(items)` as a feaLib glyph class with the same `asFea()`.
fn glyph_class(items: &[Fea]) -> GlyphClass {
	if let Some(names) = items
		.iter()
		.map(|i| i.plain_name().map(str::to_owned))
		.collect::<Option<Vec<String>>>()
	{
		return GlyphClass::new(names);
	}
	let original = items
		.iter()
		.map(|item| {
			if let Some(name) = item.plain_name() {
				GlyphClassItem::Glyph(name.to_owned())
			} else if let Some(c) = item.class_def() {
				GlyphClassItem::Class(ClassName::GlyphClassName(class_name(c)))
			} else {
				// A nested class: written as it is (`asFea()` of a written class
				// never needs escaping again).
				GlyphClassItem::Glyph(item.as_fea())
			}
		})
		.collect();
	GlyphClass {
		original,
		..Default::default()
	}
}

fn containers(items: &[Fea]) -> Vec<GlyphContainer> {
	items.iter().map(Fea::to_container).collect()
}

/// A lookup being built (`Lookup`, a `LookupBlock` with its chained lookups).
#[derive(Debug, Default)]
struct Lookup {
	name: String,
	use_extension: bool,
	statements: Vec<ast::Statement>,
	chained: Vec<Rc<ast::LookupBlock>>,
}

/// `_flatten_group(group)`: the items of a group, with enums expanded.
fn flatten_group(items: &[CoverageItem], out: &mut Vec<CoverageItem>) {
	for item in items {
		match item {
			CoverageItem::Enum(e) => flatten_group(&e.enum_, out),
			_ => out.push(item.clone()),
		}
	}
}

/// `TopologicalSorter(graph).static_order()` (`graphlib`), with its cycle
/// detection.
fn static_order(graph: &IndexMap<String, Vec<String>>) -> Result<Vec<String>> {
	struct Info {
		npredecessors: i64,
		successors: Vec<usize>,
	}
	let mut node2info: IndexMap<String, Info> = IndexMap::new();
	let info_index = |node2info: &mut IndexMap<String, Info>, node: &str| -> usize {
		match node2info.get_index_of(node) {
			Some(i) => i,
			None => {
				node2info.insert(
					node.to_owned(),
					Info {
						npredecessors: 0,
						successors: Vec::new(),
					},
				);
				node2info.len() - 1
			}
		}
	};
	for (node, predecessors) in graph {
		let n = info_index(&mut node2info, node);
		node2info[n].npredecessors += predecessors.len() as i64;
		for pred in predecessors {
			let p = info_index(&mut node2info, pred);
			node2info[p].successors.push(n);
		}
	}
	// `_find_cycle`.
	let mut seen: HashSet<usize> = HashSet::new();
	for start in 0..node2info.len() {
		if seen.contains(&start) {
			continue;
		}
		let mut stack: Vec<usize> = Vec::new();
		let mut itstack: Vec<usize> = Vec::new();
		let mut node2stacki: HashMap<usize, usize> = HashMap::new();
		let mut node = start;
		'walk: loop {
			if seen.contains(&node) {
				if let Some(&i) = node2stacki.get(&node) {
					let mut cycle: Vec<String> = stack[i..]
						.iter()
						.map(|&n| {
							fontoxide_misc::py::str_repr(node2info.get_index(n).map_or("", |e| e.0))
						})
						.collect();
					cycle.push(fontoxide_misc::py::str_repr(
						node2info.get_index(node).map_or("", |e| e.0),
					));
					return Err(Error::Python(format!(
						"graphlib.CycleError: ('nodes are in a cycle', [{}])",
						cycle.join(", ")
					)));
				}
			} else {
				seen.insert(node);
				itstack.push(0);
				node2stacki.insert(node, stack.len());
				stack.push(node);
			}
			// Backtrack to the topmost stack entry with at least another successor.
			loop {
				let (Some(&top), Some(pos)) = (stack.last(), itstack.last_mut()) else {
					break 'walk;
				};
				let successors = &node2info[top].successors;
				if *pos < successors.len() {
					node = successors[*pos];
					*pos += 1;
					break;
				}
				node2stacki.remove(&top);
				stack.pop();
				itstack.pop();
			}
		}
	}
	let mut ready: Vec<usize> = (0..node2info.len())
		.filter(|&i| node2info[i].npredecessors == 0)
		.collect();
	let mut order = Vec::new();
	while !ready.is_empty() {
		let group = std::mem::take(&mut ready);
		for &n in &group {
			node2info[n].npredecessors = -1;
		}
		for &n in &group {
			order.push(n);
			let successors = node2info[n].successors.clone();
			for s in successors {
				node2info[s].npredecessors -= 1;
				if node2info[s].npredecessors == 0 {
					ready.push(s);
				}
			}
		}
	}
	Ok(order
		.into_iter()
		.filter_map(|n| node2info.get_index(n).map(|e| e.0.clone()))
		.collect())
}

/// Topologically sorts the group definitions, so that all groups are defined
/// before they are referenced (`sort_groups`). FEA requires it but VOLT does
/// not.
fn sort_groups(groups: &[Rc<GroupDefinition>]) -> Result<Vec<Rc<GroupDefinition>>> {
	let mut group_map: HashMap<String, Rc<GroupDefinition>> = HashMap::new();
	let mut graph: IndexMap<String, Vec<String>> = IndexMap::new();
	for group in groups {
		group_map.insert(group.name.to_lowercase(), group.clone());
		let mut items = Vec::new();
		if let Some(e) = &group.enum_ {
			flatten_group(&e.enum_, &mut items);
		}
		let deps = items
			.iter()
			.filter_map(|x| match x {
				CoverageItem::GroupName(g) => Some(g.group.to_lowercase()),
				_ => None,
			})
			.collect();
		graph.insert(group.name.to_lowercase(), deps);
	}
	static_order(&graph)?
		.into_iter()
		.map(|name| {
			group_map
				.get(&name)
				.cloned()
				.ok_or_else(|| Error::key(&name))
		})
		.collect()
}

fn sanitize(name: &str, keep: impl Fn(char) -> bool) -> String {
	name.chars()
		.map(|c| if keep(c) { c } else { '_' })
		.collect()
}

/// `int or 0`.
fn or_zero(v: Option<i64>) -> i64 {
	v.unwrap_or(0)
}

/// `adjust_by and adjust_by.items() or None`.
fn device(adjust_by: &IndexMap<i64, i64>) -> Option<ast::Device> {
	if adjust_by.is_empty() {
		None
	} else {
		Some(adjust_by.iter().map(|(&s, &a)| (s, a)).collect())
	}
}

/// `seq[i]` with Python's negative indices.
fn py_index<T>(seq: &[T], i: i64) -> Result<&T> {
	let len = seq.len() as i64;
	let i = if i < 0 { i + len } else { i };
	if i < 0 {
		return Err(Error::index());
	}
	seq.get(i as usize).ok_or_else(Error::index)
}

fn assert_that(condition: bool, what: &str) -> Result<()> {
	if condition {
		Ok(())
	} else {
		Err(Error::Assertion(what.to_owned()))
	}
}

fn warn_empty_substitution(location: &Location) -> Result<()> {
	let Some(l) = location else {
		return Err(Error::Python(
			"TypeError: cannot unpack non-iterable NoneType object".to_owned(),
		));
	};
	log::warn!("{l}: Ignoring empty substitution");
	Ok(())
}

/// The input of the converter: a parsed project, or one to parse.
#[derive(Debug, Clone)]
pub enum VoltInput {
	/// A parsed project (`VoltFile`).
	Doc(VoltFile),
	/// A project to parse (a path or file object).
	Source(ParserInput),
}

impl From<VoltFile> for VoltInput {
	fn from(doc: VoltFile) -> Self {
		VoltInput::Doc(doc)
	}
}

impl From<ParserInput> for VoltInput {
	fn from(input: ParserInput) -> Self {
		VoltInput::Source(input)
	}
}

/// The converter (`VoltToFea`).
///
/// ```
/// use fontoxide_voltlib::{ParserInput, VoltToFea};
///
/// let vtp = "DEF_GLYPH \"a\" ID 1 TYPE BASE END_GLYPH END";
/// let fea = VoltToFea::new(ParserInput::text(vtp), None).convert(None, false)?;
/// assert_eq!(fea, "@GDEF_base = [a];\ntable GDEF {\n    GlyphClassDef @GDEF_base, , , ;\n} GDEF;\n");
/// # Ok::<(), fontoxide_voltlib::Error>(())
/// ```
#[derive(Debug)]
pub struct VoltToFea {
	doc: Option<VoltFile>,
	file_or_path: Option<ParserInput>,
	font_glyph_order: Option<Vec<String>>,

	glyph_map: HashMap<String, String>,
	glyph_order: Option<Vec<String>>,

	gdef: IndexMap<String, Vec<Fea>>,
	glyphclasses: IndexMap<String, Rc<ClassDef>>,
	features: IndexMap<String, IndexMap<String, IndexMap<String, Vec<String>>>>,
	lookups: IndexMap<String, Lookup>,

	marks: HashSet<String>,
	ligatures: HashMap<String, Option<i64>>,

	markclasses: BTreeMap<(String, String), ast::MarkClassDefinition>,
	mark_class_refs: IndexMap<String, MarkClassRef>,
	anchors: HashMap<String, HashMap<String, IndexMap<i64, Anchor>>>,

	settings: HashMap<String, SettingValue>,

	lookup_names: HashMap<String, String>,
	lookup_name_values: HashSet<String>,
	class_names: HashMap<String, String>,
	class_name_values: HashSet<String>,
}

impl VoltToFea {
	/// `VoltToFea(file_or_path, font)`; `glyph_order` is the glyph order of the
	/// font (`font.getGlyphOrder()`), if there is one: glyphs are then renamed to
	/// the names of their glyph IDs in the font.
	pub fn new(input: impl Into<VoltInput>, glyph_order: Option<Vec<String>>) -> Self {
		let (doc, file_or_path) = match input.into() {
			VoltInput::Doc(d) => (Some(d), None),
			VoltInput::Source(s) => (None, Some(s)),
		};
		VoltToFea {
			doc,
			file_or_path,
			font_glyph_order: glyph_order,
			glyph_map: HashMap::new(),
			glyph_order: None,
			gdef: IndexMap::new(),
			glyphclasses: IndexMap::new(),
			features: IndexMap::new(),
			lookups: IndexMap::new(),
			marks: HashSet::new(),
			ligatures: HashMap::new(),
			markclasses: BTreeMap::new(),
			mark_class_refs: IndexMap::new(),
			anchors: HashMap::new(),
			settings: HashMap::new(),
			lookup_names: HashMap::new(),
			lookup_name_values: HashSet::new(),
			class_names: HashMap::new(),
			class_name_values: HashSet::new(),
		}
	}

	/// `VoltToFea(file_or_path, font)` with a font.
	pub fn with_font(input: impl Into<VoltInput>, font: &mut Font) -> Result<Self> {
		let order = font.glyph_order()?.names().to_vec();
		Ok(Self::new(input, Some(order)))
	}

	fn lookup_name(&mut self, name: &str) -> String {
		if let Some(res) = self.lookup_names.get(name) {
			return res.clone();
		}
		let mut res = sanitize(name, |c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
		while self.lookup_name_values.contains(&res) {
			res.push('_');
		}
		self.lookup_names.insert(name.to_owned(), res.clone());
		self.lookup_name_values.insert(res.clone());
		res
	}

	fn class_name(&mut self, name: &str) -> String {
		if let Some(res) = self.class_names.get(name) {
			return res.clone();
		}
		let mut res = sanitize(name, |c| {
			c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
		});
		while self.class_name_values.contains(&res) {
			res.push('_');
		}
		self.class_names.insert(name.to_owned(), res.clone());
		self.class_name_values.insert(res.clone());
		res
	}

	fn collect_statements(
		&mut self,
		doc: &VoltFile,
		tables: &[&str],
		ignore_unsupported_settings: bool,
	) -> Result<()> {
		// Collect glyph definitions first, as we need them to map VOLT glyph names
		// to font glyph names.
		for statement in &doc.statements {
			if let VStatement::GlyphDefinition(g) = statement {
				self.glyph_definition(g)?;
			}
		}

		// Collect and sort group definitions first, to make sure a group
		// definition that references other groups comes after them since VOLT
		// does not enforce such ordering, and feature files require it.
		let groups: Vec<Rc<GroupDefinition>> = doc
			.statements
			.iter()
			.filter_map(|s| match s {
				VStatement::GroupDefinition(g) => Some(g.clone()),
				_ => None,
			})
			.collect();
		for group in sort_groups(&groups)? {
			self.group_definition(&group)?;
		}

		for statement in &doc.statements {
			match statement {
				VStatement::AnchorDefinition(a) => {
					if tables.contains(&"GPOS") {
						self.anchor_definition(a)?;
					}
				}
				VStatement::SettingDefinition(s) => {
					self.setting_definition(s, ignore_unsupported_settings);
				}
				VStatement::ScriptDefinition(s) => self.script_definition(s)?,
				VStatement::GlyphDefinition(_)
				| VStatement::GroupDefinition(_)
				| VStatement::LookupDefinition(_) => {}
			}
		}

		// Lookup definitions need to be handled last as they reference glyph and
		// mark classes that might be defined after them.
		for statement in &doc.statements {
			if let VStatement::LookupDefinition(lookup) = statement {
				if lookup.pos.is_some() && !tables.contains(&"GPOS") {
					continue;
				}
				if lookup.sub.is_some() && !tables.contains(&"GSUB") {
					continue;
				}
				self.lookup_definition(lookup)?;
			}
		}
		Ok(())
	}

	fn build_feature_file(&mut self, tables: &[&str]) -> Result<ast::FeatureFile> {
		let mut doc = ast::FeatureFile::new();
		let comment = |text: &str| {
			ast::Statement::Comment(ast::Comment {
				text: text.to_owned(),
				location: None,
			})
		};

		if !self.glyphclasses.is_empty() {
			doc.statements.push(comment("# Glyph classes"));
			for c in self.glyphclasses.values() {
				doc.statements
					.push(ast::Statement::GlyphClassDefinition(c.fea.clone()));
			}
		}

		if !self.markclasses.is_empty() {
			doc.statements.push(comment("\n# Mark classes"));
			for def in self.markclasses.values() {
				doc.statements
					.push(ast::Statement::MarkClassDefinition(Rc::new(def.clone())));
			}
		}
		for (name, mc) in &self.mark_class_refs {
			doc.mark_classes.insert(name.clone(), mc.clone());
		}

		let mut blocks: HashMap<String, Rc<ast::LookupBlock>> = HashMap::new();
		if !self.lookups.is_empty() {
			doc.statements.push(comment("\n# Lookups"));
			for (key, lookup) in &self.lookups {
				for chained in &lookup.chained {
					doc.statements
						.push(ast::Statement::LookupBlock(chained.clone()));
				}
				let block = Rc::new(ast::LookupBlock {
					name: lookup.name.clone(),
					use_extension: lookup.use_extension,
					statements: lookup.statements.clone(),
					location: None,
				});
				blocks.insert(key.clone(), block.clone());
				doc.statements.push(ast::Statement::LookupBlock(block));
			}
		}

		// Prune features
		let mut features: IndexMap<String, IndexMap<String, IndexMap<String, Vec<String>>>> =
			IndexMap::new();
		for (feature_tag, scripts) in self.features.iter_mut() {
			let mut kept_scripts = IndexMap::new();
			for (script_tag, langs) in scripts.iter_mut() {
				for lookups in langs.values_mut() {
					lookups.retain(|l| self.lookups.contains_key(&l.to_lowercase()));
				}
				langs.retain(|_, l| !l.is_empty());
				if !langs.is_empty() {
					kept_scripts.insert(script_tag.clone(), langs.clone());
				}
			}
			if !kept_scripts.is_empty() {
				features.insert(feature_tag.clone(), kept_scripts);
			}
		}

		if !features.is_empty() {
			doc.statements.push(comment("# Features"));
			for (feature_tag, scripts) in &features {
				let mut feature = ast::FeatureBlock {
					name: feature_tag.clone(),
					..Default::default()
				};
				let mut script_tags: Vec<&String> = scripts.keys().collect();
				script_tags.sort_by_key(|k| if k.as_str() == "DFLT" { 0 } else { 1 });
				if feature_tag == "aalt" && script_tags.len() > 1 {
					log::warn!(
						"FEA syntax does not allow script statements in 'aalt' feature, so only lookups from the first script will be included."
					);
					script_tags.truncate(1);
				}
				for script_tag in script_tags {
					if feature_tag != "aalt" {
						feature.statements.push(ast::Statement::ScriptStatement(
							ast::ScriptStatement {
								script: script_tag.clone(),
								location: None,
							},
						));
					}
					let langs = &scripts[script_tag];
					let mut language_tags: Vec<&String> = langs.keys().collect();
					language_tags.sort_by_key(|k| if k.as_str() == "dflt" { 0 } else { 1 });
					if feature_tag == "aalt" && language_tags.len() > 1 {
						log::warn!(
							"FEA syntax does not allow language statements in 'aalt' feature, so only lookups from the first language will be included."
						);
						language_tags.truncate(1);
					}
					for language_tag in language_tags {
						if feature_tag != "aalt" {
							let include_default = language_tag == "dflt";
							let mut tag = language_tag.clone();
							while tag.chars().count() < 4 {
								tag.push(' ');
							}
							feature.statements.push(ast::Statement::LanguageStatement(
								ast::LanguageStatement::new(
									vec![tag],
									include_default,
									false,
									None,
								)?,
							));
						}
						for name in &langs[language_tag] {
							let lookup = blocks
								.get(&name.to_lowercase())
								.ok_or_else(|| Error::key(&name.to_lowercase()))?;
							feature
								.statements
								.push(ast::Statement::LookupReferenceStatement(
									ast::LookupReferenceStatement {
										lookup: lookup.clone(),
										location: None,
									},
								));
						}
					}
				}
				doc.statements.push(ast::Statement::FeatureBlock(feature));
			}
		}

		if !self.gdef.is_empty() && tables.contains(&"GDEF") {
			let mut classes: Vec<Option<GlyphContainer>> = Vec::new();
			for name in ["BASE", "MARK", "LIGATURE", "COMPONENT"] {
				if let Some(glyphs) = self.gdef.get(name) {
					let glyphclass = Rc::new(ast::GlyphClassDefinition {
						name: format!("GDEF_{}", name.to_lowercase()),
						glyphs: GlyphContainer::GlyphClass(glyph_class(glyphs)),
						location: None,
					});
					doc.statements
						.push(ast::Statement::GlyphClassDefinition(glyphclass.clone()));
					classes.push(Some(GlyphContainer::GlyphClassName(ast::GlyphClassName {
						glyphclass,
						location: None,
					})));
				} else {
					classes.push(None);
				}
			}
			let mut classes = classes.into_iter();
			let statement = ast::GlyphClassDefStatement {
				base_glyphs: classes.next().flatten(),
				mark_glyphs: classes.next().flatten(),
				ligature_glyphs: classes.next().flatten(),
				component_glyphs: classes.next().flatten(),
				location: None,
			};
			doc.statements
				.push(ast::Statement::TableBlock(ast::TableBlock {
					name: "GDEF".to_owned(),
					statements: vec![ast::Statement::GlyphClassDefStatement(statement)],
					location: None,
				}));
		}

		Ok(doc)
	}

	/// Converts the project to a feature file (`convert`): `tables` are the
	/// tables to write (all of [`TABLES`] by default). Unsupported settings are
	/// logged unless `ignore_unsupported_settings`.
	pub fn convert(
		&mut self,
		tables: Option<&[&str]>,
		ignore_unsupported_settings: bool,
	) -> Result<String> {
		if self.doc.is_none() {
			let input = self
				.file_or_path
				.clone()
				.ok_or_else(|| Error::Python("no project to convert".to_owned()))?;
			self.doc = Some(Parser::new(input)?.parse()?);
		}
		let doc = self
			.doc
			.clone()
			.ok_or_else(|| Error::Python("no project to convert".to_owned()))?;

		let tables = tables.unwrap_or(&TABLES);
		if let Some(order) = &self.font_glyph_order {
			self.glyph_order = Some(order.clone());
		}

		self.collect_statements(&doc, tables, ignore_unsupported_settings)?;
		let fea = self.build_feature_file(tables)?;
		Ok(fea.as_fea()?)
	}

	/// `_glyphName(glyph)` for a glyph name string.
	fn glyph_name(&self, name: &str) -> Fea {
		let mapped = self.glyph_map.get(name).map_or(name, String::as_str);
		Fea::GlyphName(Box::new(Fea::Str(mapped.to_owned())))
	}

	/// `_glyphName(glyph)` for any object: the `glyph` of a `GlyphName`, or the
	/// object itself, mapped to the font's glyph name.
	// UPSTREAM-BUG(B57): alternate and exploded ligature substitutions pass the glyphs of
	// coverages that are already mapped to the font's glyph names, so with a font
	// whose names differ from the project's, names are mapped twice.
	fn glyph_name_fea(&self, glyph: &Fea) -> Fea {
		let name = match glyph {
			Fea::GlyphName(g) => (**g).clone(),
			other => other.clone(),
		};
		match name {
			Fea::Str(s) => self.glyph_name(&s),
			other => Fea::GlyphName(Box::new(other)),
		}
	}

	/// `_groupName(group)`.
	fn group_name(&self, name: &str) -> Result<Fea> {
		let key = name.to_lowercase();
		self.glyphclasses
			.get(&key)
			.map(|c| Fea::ClassName(c.clone()))
			.ok_or_else(|| Error::key(&key))
	}

	/// `_glyphSet(item)` of a range.
	fn range_glyph_set(&self, range: &crate::ast::Range) -> Result<Vec<Fea>> {
		Ok(range
			.glyph_set()?
			.iter()
			.map(|x| self.glyph_name(x))
			.collect())
	}

	/// `_coverage(coverage, flatten)`.
	fn coverage(&self, coverage: &[CoverageItem], flatten: bool) -> Result<Vec<Fea>> {
		let mut items = Vec::new();
		for item in coverage {
			match item {
				CoverageItem::GlyphName(g) => items.push(self.glyph_name(&g.glyph)),
				CoverageItem::GroupName(g) => items.push(self.group_name(&g.group)?),
				CoverageItem::Enum(e) => {
					let sub = self.coverage(&e.enum_, true)?;
					if flatten {
						items.extend(sub);
					} else {
						items.push(Fea::GlyphClass(sub));
					}
				}
				CoverageItem::Range(r) => {
					let sub = self.range_glyph_set(r)?;
					if flatten {
						items.extend(sub);
					} else {
						items.push(Fea::GlyphClass(sub));
					}
				}
			}
		}
		Ok(items)
	}

	/// `_context(context)`.
	fn context(&self, context: &[Coverage]) -> Result<Vec<Fea>> {
		let mut out = Vec::new();
		for item in context {
			let mut coverage = self.coverage(item, true)?;
			if coverage.len() > 1 {
				out.push(Fea::GlyphClass(coverage));
			} else if coverage.is_empty() {
				return Err(Error::index());
			} else {
				out.push(coverage.remove(0));
			}
		}
		Ok(out)
	}

	fn group_definition(&mut self, group: &GroupDefinition) -> Result<()> {
		let name = self.class_name(&group.name);
		let Some(e) = &group.enum_ else {
			return Err(Error::Python(
				"AttributeError: 'NoneType' object has no attribute 'enum'".to_owned(),
			));
		};
		let glyphs = self.coverage(&e.enum_, true)?;
		let fea = Rc::new(ast::GlyphClassDefinition {
			name,
			glyphs: GlyphContainer::GlyphClass(glyph_class(&glyphs)),
			location: None,
		});
		self.glyphclasses.insert(
			group.name.to_lowercase(),
			Rc::new(ClassDef { items: glyphs, fea }),
		);
		Ok(())
	}

	fn glyph_definition(&mut self, glyph: &GlyphDefinition) -> Result<()> {
		if let Some(order) = &self.glyph_order {
			let mapped = py_index(order, glyph.id)?.clone();
			self.glyph_map.insert(glyph.name.clone(), mapped);
		}

		if let Some(t) = &glyph.glyph_type
			&& ["BASE", "MARK", "LIGATURE", "COMPONENT"].contains(&t.as_str())
		{
			let name = self.glyph_name(&glyph.name);
			self.gdef.entry(t.clone()).or_default().push(name);
		}

		match glyph.glyph_type.as_deref() {
			Some("MARK") => {
				self.marks.insert(glyph.name.clone());
			}
			Some("LIGATURE") => {
				self.ligatures.insert(glyph.name.clone(), glyph.components);
			}
			_ => {}
		}
		Ok(())
	}

	fn script_definition(&mut self, script: &ScriptDefinition) -> Result<()> {
		let stag = &script.tag;
		for lang in &script.langs {
			let ltag = &lang.tag;
			for feature in &lang.features {
				let mut lookups: IndexSet<String> = IndexSet::new();
				for l in &feature.lookups {
					lookups.insert(l.split('\\').next().unwrap_or_default().to_owned());
				}
				let langs = self
					.features
					.entry(feature.tag.clone())
					.or_default()
					.entry(stag.clone())
					.or_default();
				assert_that(
					!langs.contains_key(ltag),
					"ltag not in self._features[ftag][stag]",
				)?;
				langs.insert(ltag.clone(), lookups.into_iter().collect());
			}
		}
		Ok(())
	}

	fn setting_definition(&mut self, setting: &SettingDefinition, ignore_unsupported: bool) {
		if setting.name.starts_with("COMPILER_") {
			self.settings
				.insert(setting.name.clone(), setting.value.clone());
		} else if !ignore_unsupported {
			log::warn!("Unsupported setting ignored: {}", setting.name);
		}
	}

	/// `_adjustment(adjustment)`.
	fn adjustment(adjustment: &Pos) -> ValueRecord {
		ValueRecord {
			x_placement: adjustment.dx.map(ScalarValue::Int),
			y_placement: adjustment.dy.map(ScalarValue::Int),
			x_advance: adjustment.adv.map(ScalarValue::Int),
			x_pla_device: device(&adjustment.dx_adjust_by),
			y_pla_device: device(&adjustment.dy_adjust_by),
			x_adv_device: device(&adjustment.adv_adjust_by),
			..Default::default()
		}
	}

	/// `_anchor(adjustment)`.
	fn anchor(adjustment: &Pos) -> Result<Anchor> {
		assert_that(adjustment.adv_adjust_by.is_empty(), "not adv_adjust_by")?;
		let mut anchor = Anchor::new(or_zero(adjustment.dx), or_zero(adjustment.dy));
		anchor.x_device_table = device(&adjustment.dx_adjust_by);
		anchor.y_device_table = device(&adjustment.dy_adjust_by);
		Ok(anchor)
	}

	fn anchor_definition(&mut self, anchordef: &AnchorDefinition) -> Result<()> {
		let anchor = Self::anchor(&anchordef.pos)?;
		let anchorname = match anchordef.name.strip_prefix("MARK_") {
			Some(rest) => format!("MARK_{}", rest.to_lowercase()),
			None => anchordef.name.to_lowercase(),
		};
		self.anchors
			.entry(anchordef.glyph_name.clone())
			.or_default()
			.entry(anchorname)
			.or_default()
			.insert(anchordef.component, anchor);
		Ok(())
	}

	fn glyph_anchor(&self, glyph: &str, anchor: &str, component: i64) -> Result<Anchor> {
		self.anchors
			.get(glyph)
			.ok_or_else(|| Error::key(glyph))?
			.get(anchor)
			.ok_or_else(|| Error::key(anchor))?
			.get(&component)
			.cloned()
			.ok_or_else(|| Error::Python(format!("KeyError: {component}")))
	}

	fn mark_class(&mut self, name: &str) -> MarkClassRef {
		self.mark_class_refs
			.entry(name.to_owned())
			.or_insert_with(|| MarkClass::new(name))
			.clone()
	}

	fn gpos_lookup(
		&mut self,
		lookup: &LookupDefinition,
		statements: &mut Vec<ast::Statement>,
	) -> Result<()> {
		let Some(pos) = &lookup.pos else {
			return Ok(());
		};
		match pos {
			PositionDefinition::AdjustPair(pos) => {
				for (&(idx1, idx2), (pos1, pos2)) in &pos.adjust_pair {
					let coverage_1 = py_index(&pos.coverages_1, idx1 - 1)?;
					let coverage_2 = py_index(&pos.coverages_2, idx2 - 1)?;

					// If not both are groups, use "enum pos" otherwise makeotf will
					// fail.
					let enumerated = coverage_1
						.iter()
						.chain(coverage_2)
						.any(|item| !matches!(item, CoverageItem::GroupName(_)));

					let glyphs1 = self.coverage(coverage_1, false)?;
					let glyphs2 = self.coverage(coverage_2, false)?;
					let record1 = Self::adjustment(pos1);
					let record2 = Self::adjustment(pos2);
					assert_that(glyphs1.len() == 1, "len(glyphs1) == 1")?;
					assert_that(glyphs2.len() == 1, "len(glyphs2) == 1")?;
					statements.push(ast::Statement::PairPosStatement(ast::PairPosStatement {
						glyphs1: glyphs1[0].to_container(),
						valuerecord1: Some(record1),
						glyphs2: glyphs2[0].to_container(),
						valuerecord2: Some(record2),
						enumerated,
						location: None,
					}));
				}
			}
			PositionDefinition::AdjustSingle(pos) => {
				for (a, b) in &pos.adjust_single {
					let glyphs = self.coverage(a, false)?;
					let record = Self::adjustment(b);
					assert_that(glyphs.len() == 1, "len(glyphs) == 1")?;
					statements.push(ast::Statement::SinglePosStatement(
						ast::SinglePosStatement {
							pos: vec![(glyphs[0].to_container(), Some(record))],
							prefix: Vec::new(),
							suffix: Vec::new(),
							force_chain: false,
							location: None,
						},
					));
				}
			}
			PositionDefinition::Attach(pos) => {
				let mut anchors: IndexMap<String, Vec<(String, String)>> = IndexMap::new();
				let mut allmarks: HashSet<String> = HashSet::new();
				for (coverage, anchorname) in &pos.coverage_to {
					// In feature files mark classes are global, but in VOLT they
					// are defined per-lookup. If we output mark class definitions
					// for all marks that use a given anchor, we might end up with a
					// mark used in two different classes in the same lookup, which
					// causes a feature file compilation error.
					// At the expense of uglier feature code, we make the mark class
					// name by appending the current lookup name not the anchor
					// name, and output mark class definitions only for marks used
					// in this lookup.
					let classname = self.class_name(&format!("{anchorname}.{}", lookup.name));
					let markclass = self.mark_class(&classname);

					// Anchor names are case-insensitive in VOLT
					let anchorname = anchorname.to_lowercase();

					// We might still end in marks used in two different anchor
					// classes, so we filter out already used marks.
					let mut marks: IndexSet<String> = IndexSet::new();
					for mark in coverage {
						marks.extend(mark.glyph_set()?);
					}
					if marks.iter().any(|m| allmarks.contains(m)) {
						marks.retain(|m| !allmarks.contains(m));
						if marks.is_empty() {
							continue;
						}
					}
					allmarks.extend(marks.iter().cloned());

					// The order of `marks` (a set in Python) does not matter: the
					// definitions are sorted by glyph and class name.
					for glyphname in &marks {
						let glyph = self.glyph_name(glyphname);
						let anchor =
							self.glyph_anchor(glyphname, &format!("MARK_{anchorname}"), 1)?;
						let markdef = ast::MarkClassDefinition::new(
							&markclass,
							anchor,
							glyph.to_container(),
							None,
						);
						self.markclasses
							.insert((glyphname.clone(), classname.clone()), markdef);
					}

					for base in &pos.coverage {
						for name in base.glyph_set()? {
							let entry = anchors.entry(name).or_default();
							let pair = (anchorname.clone(), classname.clone());
							if !entry.contains(&pair) {
								entry.push(pair);
							}
						}
					}
				}

				let is_ligature = anchors.keys().all(|n| self.ligatures.contains_key(n));
				let is_mark = anchors.keys().all(|n| self.marks.contains(n));
				for (name, name_anchors) in &anchors {
					let mut components = 1;
					if is_ligature {
						components =
							self.ligatures.get(name).copied().flatten().ok_or_else(|| {
								Error::Python(
									"TypeError: 'NoneType' object cannot be interpreted as an integer"
										.to_owned(),
								)
							})?;
					}

					let mut marks: Vec<AnchorMarks> =
						(0..components.max(0)).map(|_| Vec::new()).collect();
					for (mark, classname) in name_anchors {
						let markclass = self.mark_class(classname);
						let glyph_anchors = self
							.anchors
							.get(name)
							.ok_or_else(|| Error::key(name))?
							.get(mark)
							.ok_or_else(|| Error::key(mark))?;
						for component in 1..=components {
							if let Some(anchor) = glyph_anchors.get(&component) {
								marks[(component - 1) as usize]
									.push((Some(anchor.clone()), markclass.clone()));
							}
						}
					}

					let base = self.glyph_name(name).to_container();
					let first = || marks.first().cloned().ok_or_else(Error::index);
					let statement = if is_mark {
						ast::Statement::MarkMarkPosStatement(ast::MarkMarkPosStatement {
							base_marks: base,
							marks: first()?,
							location: None,
						})
					} else if is_ligature {
						ast::Statement::MarkLigPosStatement(ast::MarkLigPosStatement {
							ligatures: base,
							marks: marks.clone(),
							location: None,
						})
					} else {
						ast::Statement::MarkBasePosStatement(ast::MarkBasePosStatement {
							base,
							marks: first()?,
							location: None,
						})
					};
					statements.push(statement);
				}
			}
			PositionDefinition::AttachCursive(pos) => {
				// Collect enter and exit glyphs
				let mut enter_coverage = Vec::new();
				for coverage in &pos.coverages_enter {
					for base in coverage {
						enter_coverage.extend(base.glyph_set()?);
					}
				}
				let mut exit_coverage = Vec::new();
				for coverage in &pos.coverages_exit {
					for base in coverage {
						exit_coverage.extend(base.glyph_set()?);
					}
				}

				// Write enter anchors, also check if the glyph has exit anchor and
				// write it, too.
				for name in &enter_coverage {
					let glyph = self.glyph_name(name);
					let entry = self.glyph_anchor(name, "entry", 1)?;
					let mut exit = None;
					if let Some(i) = exit_coverage.iter().position(|n| n == name) {
						exit = Some(self.glyph_anchor(name, "exit", 1)?);
						exit_coverage.remove(i);
					}
					statements.push(ast::Statement::CursivePosStatement(
						ast::CursivePosStatement {
							glyphclass: glyph.to_container(),
							entry_anchor: Some(entry),
							exit_anchor: exit,
							location: None,
						},
					));
				}

				// Write any remaining exit anchors.
				for name in &exit_coverage {
					let glyph = self.glyph_name(name);
					let exit = self.glyph_anchor(name, "exit", 1)?;
					statements.push(ast::Statement::CursivePosStatement(
						ast::CursivePosStatement {
							glyphclass: glyph.to_container(),
							entry_anchor: None,
							exit_anchor: Some(exit),
							location: None,
						},
					));
				}
			}
		}
		Ok(())
	}

	#[allow(clippy::too_many_arguments)]
	fn gpos_context_lookup(
		&mut self,
		lookup: &LookupDefinition,
		prefix: &[Fea],
		suffix: &[Fea],
		ignore: bool,
		fealookup: &mut Lookup,
		chained: &Rc<ast::LookupBlock>,
	) -> Result<()> {
		let Some(pos) = &lookup.pos else {
			return Ok(());
		};
		let prefix = containers(prefix);
		let suffix = containers(suffix);
		let chain = |glyphs: Vec<GlyphContainer>, n: usize| {
			if ignore {
				ast::Statement::IgnorePosStatement(ast::IgnorePosStatement {
					chain_contexts: vec![(prefix.clone(), glyphs, suffix.clone())],
					location: None,
				})
			} else {
				ast::Statement::ChainContextPosStatement(ast::ChainContextPosStatement {
					prefix: prefix.clone(),
					glyphs,
					suffix: suffix.clone(),
					lookups: vec![Some(vec![chained.clone()]); n],
					location: None,
				})
			}
		};
		match pos {
			PositionDefinition::AdjustPair(pos) => {
				for &(idx1, idx2) in pos.adjust_pair.keys() {
					let glyphs1 = self.coverage(py_index(&pos.coverages_1, idx1 - 1)?, false)?;
					let glyphs2 = self.coverage(py_index(&pos.coverages_2, idx2 - 1)?, false)?;
					assert_that(glyphs1.len() == 1, "len(glyphs1) == 1")?;
					assert_that(glyphs2.len() == 1, "len(glyphs2) == 1")?;
					let glyphs = vec![glyphs1[0].to_container(), glyphs2[0].to_container()];
					fealookup.statements.push(chain(glyphs, 2));
				}
			}
			PositionDefinition::AdjustSingle(pos) => {
				let mut glyphs = Vec::new();
				for (a, _) in &pos.adjust_single {
					glyphs.extend(self.coverage(a, true)?);
				}
				let glyphs = vec![GlyphContainer::GlyphClass(glyph_class(&glyphs))];
				fealookup.statements.push(chain(glyphs, 1));
			}
			PositionDefinition::Attach(pos) => {
				let mut glyphs = Vec::new();
				for (coverage, _) in &pos.coverage_to {
					glyphs.extend(self.coverage(coverage, true)?);
				}
				let glyphs = vec![GlyphContainer::GlyphClass(glyph_class(&glyphs))];
				fealookup.statements.push(chain(glyphs, 1));
			}
			PositionDefinition::AttachCursive(p) => {
				return Err(Error::NotImplemented {
					text: p.to_string(),
					location: p.location.clone(),
				});
			}
		}
		Ok(())
	}

	fn gsub_lookup(
		&mut self,
		lookup: &LookupDefinition,
		statements: &mut Vec<ast::Statement>,
	) -> Result<()> {
		let Some(sub) = &lookup.sub else {
			return Ok(());
		};

		// Alternate substitutions are represented by adding multiple
		// substitutions for the same glyph, so we need to collect them into one
		// to many mapping.
		if sub.kind == SubstitutionKind::Alternate {
			let mut alternates: IndexMap<String, Vec<String>> = IndexMap::new();
			for (key, val) in &sub.mapping {
				if key.is_empty() || val.is_empty() {
					warn_empty_substitution(&sub.location)?;
					continue;
				}
				let glyphs = self.coverage(key, false)?;
				let replacements = self.coverage(val, false)?;
				assert_that(glyphs.len() == 1, "len(glyphs) == 1")?;
				let src = glyphs[0].glyph_set()?;
				let repl = replacements.first().ok_or_else(Error::index)?.glyph_set()?;
				for (src_glyph, repl_glyph) in src.iter().zip(&repl) {
					alternates
						.entry(self.glyph_name_fea(src_glyph).as_fea())
						.or_default()
						.push(self.glyph_name_fea(repl_glyph).as_fea());
				}
			}

			for (glyph, replacements) in alternates {
				statements.push(ast::Statement::AlternateSubstStatement(
					ast::AlternateSubstStatement {
						prefix: Vec::new(),
						glyph: GlyphContainer::GlyphName(ast::GlyphName::new(glyph)),
						suffix: Vec::new(),
						replacement: GlyphContainer::GlyphClass(GlyphClass::new(replacements)),
						location: None,
					},
				));
			}
			return Ok(());
		}

		for (key, val) in &sub.mapping {
			if key.is_empty() || val.is_empty() {
				warn_empty_substitution(&sub.location)?;
				continue;
			}
			let glyphs = self.coverage(key, false)?;
			let replacements = self.coverage(val, false)?;
			match sub.kind {
				SubstitutionKind::Single => {
					assert_that(glyphs.len() == 1, "len(glyphs) == 1")?;
					assert_that(replacements.len() == 1, "len(replacements) == 1")?;
					statements.push(ast::Statement::SingleSubstStatement(
						ast::SingleSubstStatement {
							prefix: Vec::new(),
							suffix: Vec::new(),
							force_chain: false,
							glyphs: containers(&glyphs),
							replacements: containers(&replacements),
							location: None,
						},
					));
				}
				SubstitutionKind::ReverseChainingSingle => {
					// This is handled in gsub_context_lookup()
				}
				SubstitutionKind::Multiple => {
					assert_that(glyphs.len() == 1, "len(glyphs) == 1")?;
					statements.push(ast::Statement::MultipleSubstStatement(
						ast::MultipleSubstStatement {
							prefix: Vec::new(),
							glyph: glyphs[0].to_container(),
							suffix: Vec::new(),
							replacement: containers(&replacements),
							force_chain: false,
							location: None,
						},
					));
				}
				SubstitutionKind::Ligature => {
					assert_that(replacements.len() == 1, "len(replacements) == 1")?;
					let statement = ligature(&glyphs, &replacements[0]);

					// If any of the input glyphs is a group, we need to explode
					// the substitution into multiple ligature substitutions since
					// feature file syntax does not support classes in ligature
					// substitutions.
					let sets: Vec<Vec<Fea>> =
						glyphs.iter().map(Fea::glyph_set).collect::<Result<_>>()?;
					let n = sets.iter().map(Vec::len).max().unwrap_or(0);
					if n > 1 {
						// All input should either be groups of the same length or
						// single glyphs
						assert_that(
							sets.iter().all(|x| x.len() == n || x.len() == 1),
							"all(len(x.glyphSet()) in (n, 1) for x in glyphs)",
						)?;
						let sets: Vec<Vec<Fea>> = sets
							.into_iter()
							.map(|x| {
								if x.len() == 1 {
									vec![x[0].clone(); n]
								} else {
									x
								}
							})
							.collect();

						// In this case ligature replacements must be a group of
						// the same length as the input groups, or a single glyph.
						// VOLT allows the replacement glyphs to be longer and
						// truncates them. So we'll allow that and zip() below will
						// do the truncation for us.
						let mut replacement = replacements[0].glyph_set()?;
						if replacement.len() == 1 {
							replacement = vec![replacement[0].clone(); n];
						}
						assert_that(replacement.len() >= n, "len(replacement) >= n")?;

						// Add the unexploded statement commented out for reference.
						statements.push(ast::Statement::Comment(ast::Comment {
							text: format!("# {}", statement.as_fea()),
							location: None,
						}));

						let rows = sets
							.iter()
							.map(Vec::len)
							.chain([replacement.len()])
							.min()
							.unwrap_or(0);
						for i in 0..rows {
							let mut zipped: Vec<Fea> =
								sets.iter().map(|s| self.glyph_name_fea(&s[i])).collect();
							let last = self.glyph_name_fea(&replacement[i]);
							zipped.push(last);
							let (lig, components) = zipped.split_last().ok_or_else(Error::index)?;
							statements.push(ast::Statement::LigatureSubstStatement(ligature(
								components, lig,
							)));
						}
					} else {
						statements.push(ast::Statement::LigatureSubstStatement(statement));
					}
				}
				SubstitutionKind::Alternate => {}
			}
		}
		Ok(())
	}

	#[allow(clippy::too_many_arguments)]
	fn gsub_context_lookup(
		&mut self,
		lookup: &LookupDefinition,
		prefix: &[Fea],
		suffix: &[Fea],
		ignore: bool,
		fealookup: &mut Lookup,
		chained: &Rc<ast::LookupBlock>,
	) -> Result<()> {
		let Some(sub) = &lookup.sub else {
			return Ok(());
		};

		if sub.kind == SubstitutionKind::ReverseChainingSingle {
			// Reverse substitutions is a special case, it can't use chained
			// lookups.
			for (key, val) in &sub.mapping {
				if key.is_empty() || val.is_empty() {
					warn_empty_substitution(&sub.location)?;
					continue;
				}
				let glyphs = self.coverage(key, false)?;
				let replacements = self.coverage(val, false)?;
				fealookup
					.statements
					.push(ast::Statement::ReverseChainSingleSubstStatement(
						ast::ReverseChainSingleSubstStatement {
							old_prefix: containers(prefix),
							old_suffix: containers(suffix),
							glyphs: containers(&glyphs),
							replacements: containers(&replacements),
							location: None,
						},
					));
			}
			fealookup.chained = Vec::new();
			return Ok(());
		}

		let mut glyphs = Vec::new();
		for (key, val) in &sub.mapping {
			if key.is_empty() || val.is_empty() {
				warn_empty_substitution(&sub.location)?;
				continue;
			}
			glyphs.extend(self.coverage(key, true)?);
		}

		if glyphs.len() > 1 {
			glyphs = vec![Fea::GlyphClass(glyphs)];
		}
		let glyphs = containers(&glyphs);
		if ignore {
			fealookup
				.statements
				.push(ast::Statement::IgnoreSubstStatement(
					ast::IgnoreSubstStatement {
						chain_contexts: vec![(containers(prefix), glyphs, containers(suffix))],
						location: None,
					},
				));
		} else {
			fealookup
				.statements
				.push(ast::Statement::ChainContextSubstStatement(
					ast::ChainContextSubstStatement {
						prefix: containers(prefix),
						glyphs,
						suffix: containers(suffix),
						lookups: vec![Some(vec![chained.clone()])],
						location: None,
					},
				));
		}
		Ok(())
	}

	fn lookup_definition(&mut self, lookup: &LookupDefinition) -> Result<()> {
		let mut mark_attachement = None;
		let mut mark_filtering = None;

		let mut flags = 0;
		if lookup.direction.as_deref() == Some("RTL") {
			flags |= 1;
		}
		if !lookup.process_base {
			flags |= 2;
		}
		// FIXME: Does VOLT support this?
		// if not lookup.process_ligatures:
		//     flags |= 4
		if !lookup.process_marks.is_truthy() {
			flags |= 8;
		} else if let ProcessMarks::Group(g) = &lookup.process_marks {
			mark_attachement = Some(self.group_name(g)?.to_container());
		} else if let Some(set) = &lookup.mark_glyph_set {
			mark_filtering = Some(self.group_name(set)?.to_container());
		}

		let mut lookupflags = None;
		if flags != 0 || mark_attachement.is_some() || mark_filtering.is_some() {
			lookupflags = Some(ast::Statement::LookupFlagStatement(
				ast::LookupFlagStatement {
					value: flags,
					mark_attachment: mark_attachement,
					mark_filtering_set: mark_filtering,
					location: None,
				},
			));
		}

		let use_extension = match self.settings.get("COMPILER_USEEXTENSIONLOOKUPS") {
			Some(SettingValue::True) => true,
			Some(SettingValue::Number(n)) => *n != 0,
			Some(SettingValue::Numbers(v)) => !v.is_empty(),
			None => false,
		};

		let comment = |text: String| {
			ast::Statement::Comment(ast::Comment {
				text,
				location: None,
			})
		};

		let key;
		let mut fealookup;
		if lookup.name.contains('\\') {
			// Merge sub lookups as subtables (lookups named "base\sub"),
			// makeotf/feaLib will issue a warning and ignore the subtable
			// statement if it is not a pairpos lookup, though.
			let name = lookup.name.split('\\').next().unwrap_or_default();
			key = name.to_lowercase();
			match self.lookups.get_mut(&key) {
				None => {
					fealookup = Lookup {
						name: self.lookup_name(name),
						use_extension,
						..Default::default()
					};
					if let Some(f) = lookupflags {
						fealookup.statements.push(f);
					}
					fealookup
						.statements
						.push(comment(format!("# {}", lookup.name)));
				}
				Some(existing) => {
					fealookup = std::mem::take(existing);
					fealookup
						.statements
						.push(ast::Statement::SubtableStatement(Default::default()));
					fealookup
						.statements
						.push(comment(format!("# {}", lookup.name)));
				}
			}
		} else {
			key = lookup.name.to_lowercase();
			fealookup = Lookup {
				name: self.lookup_name(&lookup.name),
				use_extension,
				..Default::default()
			};
			if let Some(f) = lookupflags {
				fealookup.statements.push(f);
			}
		}

		let result = self.fill_lookup(lookup, &mut fealookup, use_extension);
		self.lookups.insert(key, fealookup);
		result
	}

	fn fill_lookup(
		&mut self,
		lookup: &LookupDefinition,
		fealookup: &mut Lookup,
		use_extension: bool,
	) -> Result<()> {
		if let Some(comments) = &lookup.comments {
			fealookup
				.statements
				.push(ast::Statement::Comment(ast::Comment {
					text: format!("# {comments}"),
					location: None,
				}));
		}

		let mut contexts = Vec::new();
		for context in &lookup.context {
			let prefix = self.context(&context.left)?;
			let suffix = self.context(&context.right)?;
			let ignore = context.ex_or_in == "EXCEPT_CONTEXT";
			contexts.push((prefix, suffix, ignore));
			// It seems that VOLT will create contextual substitution using only
			// the input if there is no other contexts in this lookup.
			if ignore && lookup.context.len() == 1 {
				contexts.push((Vec::new(), Vec::new(), false));
			}
		}

		if !contexts.is_empty() {
			let mut chained = ast::LookupBlock {
				name: self.lookup_name(&format!("{} chained", lookup.name)),
				use_extension,
				..Default::default()
			};
			if lookup.sub.is_some() {
				self.gsub_lookup(lookup, &mut chained.statements)?;
			} else if lookup.pos.is_some() {
				self.gpos_lookup(lookup, &mut chained.statements)?;
			}
			let chained = Rc::new(chained);
			fealookup.chained.push(chained.clone());
			for (prefix, suffix, ignore) in &contexts {
				if lookup.sub.is_some() {
					self.gsub_context_lookup(lookup, prefix, suffix, *ignore, fealookup, &chained)?;
				} else if lookup.pos.is_some() {
					self.gpos_context_lookup(lookup, prefix, suffix, *ignore, fealookup, &chained)?;
				}
			}
		} else if lookup.sub.is_some() {
			self.gsub_lookup(lookup, &mut fealookup.statements)?;
		} else if lookup.pos.is_some() {
			self.gpos_lookup(lookup, &mut fealookup.statements)?;
		}
		Ok(())
	}
}

/// `LigatureSubstStatement([], glyphs, [], replacement, False)`.
fn ligature(glyphs: &[Fea], replacement: &Fea) -> ast::LigatureSubstStatement {
	ast::LigatureSubstStatement {
		prefix: Vec::new(),
		glyphs: containers(glyphs),
		suffix: Vec::new(),
		// The replacement is written with `asFea()`, which plain names need and
		// which leaves anything else as it is.
		replacement: match replacement.plain_name() {
			Some(name) => name.to_owned(),
			None => replacement.as_fea(),
		},
		force_chain: false,
		location: None,
	}
}

const HELP: &str = "\
usage: fonttools voltLib.voltToFea [-h] [-t {GDEF,GSUB,GPOS}] [-q] [--traceback] INPUT OUTPUT

Convert MS VOLT to AFDKO feature files.

positional arguments:
  INPUT                 input font/VTP file to process
  OUTPUT                output feature file

options:
  -h, --help            show this help message and exit
  -t, --table {GDEF,GSUB,GPOS}
                        List of tables to write, by default all tables are written
  -q, --quiet           Suppress non-error messages
  --traceback           Don’t catch exceptions
";

/// Reads the VOLT source of `path`: the `TSIV` table if it is a font.
/// `Ok(None)` if the font has no `TSIV` table (which is logged).
pub(crate) fn read_input(
	path: &str,
	missing_tsiv_message: &str,
) -> Result<Option<(ParserInput, Option<Font>)>> {
	let data = std::fs::read(path).map_err(|e| Error::io(path, e))?;
	match Font::read(data, fontoxide_ttlib::ReadOptions::default()) {
		Ok(mut font) => {
			let tag = Tag::new(b"TSIV");
			if !font.contains(tag) {
				log::error!("{missing_tsiv_message}");
				return Ok(None);
			}
			let table = font
				.get_at::<fontoxide_ttlib::tables::AsciiTable>(tag)?
				.ok_or_else(|| Error::key("TSIV"))?;
			let text = String::from_utf8(table.data.clone()).map_err(|e| {
				Error::Python(format!(
					"UnicodeDecodeError: 'utf-8' codec can't decode byte 0x{:02x} in position {}: invalid start byte",
					e.as_bytes().get(e.utf8_error().valid_up_to()).copied().unwrap_or_default(),
					e.utf8_error().valid_up_to()
				))
			})?;
			Ok(Some((ParserInput::text(text), Some(font))))
		}
		// `TTLibError`: not a font, so a VOLT project.
		Err(_) => Ok(Some((ParserInput::Path(path.into()), None))),
	}
}

/// Logs a `NotImplementedError` as the command lines do.
pub(crate) fn log_not_implemented(text: &str, location: &Location) {
	let message = format!("\"{text}\" is not supported");
	match location {
		Some(l) => log::error!("{l}: {message}"),
		None => log::error!("{message}"),
	}
}

/// The command line `fonttools voltLib.voltToFea` (`main`): converts a VOLT
/// project, or a font with a `TSIV` table, to a feature file. Returns the exit
/// code: 1 if the font has no `TSIV` table or the project uses something that
/// cannot be converted (which is logged), unless `--traceback` asks for the
/// error to be returned.
///
/// The verbosity options are accepted; the caller configures logging.
// LENIENT(L42): the feature file is written with "\n" line breaks; Python writes in
// text mode, which is "\r\n" on Windows.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut positional: Vec<&str> = Vec::new();
	let mut tables: Option<Vec<&str>> = None;
	let mut traceback = false;
	let mut i = 0;
	while i < args.len() {
		let arg = args[i];
		i += 1;
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			_ => (arg, None),
		};
		match name {
			"-h" | "--help" => {
				print!("{HELP}");
				return Ok(0);
			}
			"-t" | "--table" => {
				let value = match inline {
					Some(v) => v,
					None => {
						let v = args.get(i).copied().ok_or_else(|| {
							Error::Usage(
								"error: argument -t/--table: expected one argument".to_owned(),
							)
						})?;
						i += 1;
						v
					}
				};
				if !TABLES.contains(&value) {
					return Err(Error::Usage(format!(
						"error: argument -t/--table: invalid choice: {} (choose from GDEF, GSUB, GPOS)",
						fontoxide_misc::py::str_repr(value)
					)));
				}
				tables.get_or_insert_with(Vec::new).push(value);
			}
			"-q" | "--quiet" => {}
			"--traceback" => traceback = true,
			_ if arg.len() > 1 && arg.starts_with('-') => {
				return Err(Error::Usage(format!(
					"error: unrecognized arguments: {arg}"
				)));
			}
			_ => positional.push(arg),
		}
	}
	let (input, featurefile) = match positional.as_slice() {
		[input, featurefile] => (*input, *featurefile),
		[] => {
			return Err(Error::Usage(
				"error: the following arguments are required: INPUT, OUTPUT".to_owned(),
			));
		}
		[_] => {
			return Err(Error::Usage(
				"error: the following arguments are required: OUTPUT".to_owned(),
			));
		}
		[_, _, rest @ ..] => {
			return Err(Error::Usage(format!(
				"error: unrecognized arguments: {}",
				rest.join(" ")
			)));
		}
	};

	let Some((file_or_path, mut font)) = read_input(
		input,
		"\"TSIV\" table is missing, font was not saved from VOLT?",
	)?
	else {
		return Ok(1);
	};

	let mut converter = match &mut font {
		Some(font) => VoltToFea::with_font(file_or_path, font)?,
		None => VoltToFea::new(file_or_path, None),
	};
	let fea = match converter.convert(tables.as_deref(), false) {
		Ok(fea) => fea,
		Err(Error::NotImplemented { text, location }) if !traceback => {
			log_not_implemented(&text, &location);
			return Ok(1);
		}
		Err(e) => return Err(e),
	};
	std::fs::write(featurefile, fea).map_err(|e| Error::io(featurefile, e))?;
	Ok(0)
}
