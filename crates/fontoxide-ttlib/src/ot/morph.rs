//! AAT finite-state machines of `mort` and `morx` (`MorphStateTable`, with the
//! `STXHeader` and `StateHeader` converters, and the `AATAction` classes).
//!
//! fontTools reads both encodings into the same `AATStateTable` and always
//! writes TTX in one form.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use fontoxide_misc::text_tools::{pad, safe_eval_int};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::aat::{AatLookup, read_aat_lookup, read_aat_lookup_xml, write_aat_lookup};
use super::table::{OtTable, XmlCtx};
use super::{CountRef, OtReader, OtWriter, StateValue, WriterGraph};
use crate::{Error, Font, Result};

/// The kind of actions a state table performs (the converter's `tableClass`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
	/// `RearrangementMorphAction`.
	Rearrangement,
	/// `ContextualMorphAction`.
	Contextual,
	/// `LigatureMorphAction`.
	Ligature,
	/// `InsertionMorphAction`.
	Insertion,
}

impl ActionKind {
	/// The size of an entry (`staticSize`).
	fn static_size(self) -> usize {
		match self {
			ActionKind::Rearrangement => 4,
			ActionKind::Ligature => 6,
			_ => 8,
		}
	}

	/// The size of the action offsets in the extended header (`actionHeaderSize`).
	fn action_header_size(self) -> usize {
		match self {
			ActionKind::Ligature => 12,
			ActionKind::Insertion => 4,
			_ => 0,
		}
	}

	fn flag_names(self) -> &'static [&'static str] {
		match self {
			ActionKind::Rearrangement => &["MarkFirst", "DontAdvance", "MarkLast"],
			ActionKind::Contextual => &["SetMark", "DontAdvance"],
			ActionKind::Ligature => &["SetComponent", "DontAdvance"],
			ActionKind::Insertion => &[
				"SetMark",
				"DontAdvance",
				"CurrentIsKashidaLike",
				"MarkedIsKashidaLike",
				"CurrentInsertBefore",
				"MarkedInsertBefore",
			],
		}
	}
}

/// A ligature action (`LigAction`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LigAction {
	/// Whether the ligature is stored (`Store`).
	pub store: bool,
	/// Added to the glyph ID on the stack to index the components
	/// (`GlyphIndexDelta`).
	pub glyph_index_delta: i64,
}

/// A transition of a state table (`AATAction` and its subclasses). The fields of
/// other kinds of actions are left at their defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct AatAction {
	/// What kind of action this is.
	pub kind: ActionKind,
	/// The next state (`NewState`).
	pub new_state: i64,
	/// The flags of [`ActionKind::flag_names`], in that order.
	pub flags: Vec<bool>,
	/// `ReservedFlags`.
	pub reserved_flags: i64,
	/// `Verb` (rearrangement).
	pub verb: i64,
	/// `MarkIndex` (contextual).
	pub mark_index: i64,
	/// `CurrentIndex` (contextual).
	pub current_index: i64,
	/// `Actions` (ligature).
	pub actions: Vec<LigAction>,
	/// `CurrentInsertionAction`, glyph IDs (insertion).
	pub current_insertion_action: Vec<u32>,
	/// `MarkedInsertionAction`, glyph IDs (insertion).
	pub marked_insertion_action: Vec<u32>,
}

impl AatAction {
	/// A new action with fontTools' defaults (`__init__`).
	pub fn new(kind: ActionKind) -> Self {
		AatAction {
			kind,
			new_state: 0,
			flags: vec![false; kind.flag_names().len()],
			reserved_flags: 0,
			verb: 0,
			mark_index: 0xFFFF,
			current_index: 0xFFFF,
			actions: Vec::new(),
			current_insertion_action: Vec::new(),
			marked_insertion_action: Vec::new(),
		}
	}

	fn flag(&self, i: usize) -> bool {
		self.flags.get(i).copied().unwrap_or(false)
	}

	/// Writes the extended (`morx`) entry (`compile`); `index` is what
	/// [`compile_actions`] returned for the state table.
	pub fn compile(&self, index: Option<&ActionIndex>) -> Result<Vec<u8>> {
		compile_action(self, index)
	}

	/// Reads an extended (`morx`) entry (`decompile`); ligature and insertion
	/// actions read their action data from `action_reader`.
	pub fn decompile(
		kind: ActionKind,
		r: &mut OtReader<'_>,
		action_reader: Option<&OtReader<'_>>,
	) -> Result<Self> {
		decompile_action(kind, r, action_reader)
	}

	/// Writes the TTX element (`toXML`).
	pub fn to_xml(
		&self,
		xw: &mut XmlWriter,
		font: &mut Font,
		attrs: &fontoxide_misc::xml_writer::Attrs<'_>,
		name: &str,
	) -> Result<()> {
		action_to_xml(self, xw, font, attrs, name)
	}

	/// Reads an action from the children of its TTX element (`fromXML`).
	pub fn from_xml(kind: ActionKind, element: &Element, font: &mut Font) -> Result<Self> {
		action_from_xml(kind, element, font)
	}
}

/// A state (`AATState`): the transition taken for each glyph class.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AatState {
	/// Transitions by glyph class (`Transitions`).
	pub transitions: BTreeMap<i64, AatAction>,
}

/// A finite-state machine (`AATStateTable`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AatStateTable {
	/// The number of glyph classes (`GlyphClassCount`).
	pub glyph_class_count: i64,
	/// The class of each glyph (`GlyphClasses`).
	pub glyph_classes: BTreeMap<u32, i64>,
	/// The states (`States`).
	pub states: Vec<AatState>,
	/// Glyph substitutions of contextual actions (`PerGlyphLookups`).
	pub per_glyph_lookups: Vec<BTreeMap<u32, u32>>,
	/// Ligature components (`LigComponents`), if present.
	pub lig_components: Option<Vec<i64>>,
	/// Ligature glyphs (`Ligatures`), if present.
	pub ligatures: Option<Vec<u32>>,
	/// The `mort` ligature components that include the ligature table offset
	/// (`_MortLigatureRebase`), if present.
	pub mort_ligature_rebase: Option<BTreeSet<i64>>,
}

fn ushort(v: i64) -> Result<u16> {
	u16::try_from(v).map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))
}

fn ulong(v: i64) -> Result<u32> {
	u32::try_from(v)
		.map_err(|_| Error::Struct("'L' format requires 0 <= number <= 4294967295".into()))
}

fn assertion(msg: impl Into<String>) -> Error {
	Error::Assertion(msg.into())
}

/// A reader at an absolute position of `r`'s data (`getSubReader(0)` + `seek`).
fn reader_at<'a>(r: &OtReader<'a>, pos: usize) -> OtReader<'a> {
	let mut s = r.sub_reader(0);
	s.seek(pos);
	s
}

/// Reads a 32-bit offset from `pos` and returns a reader there.
fn offset_reader<'a>(r: &mut OtReader<'a>, pos: usize) -> Result<OtReader<'a>> {
	let offset = r.read_u32()? as usize;
	Ok(reader_at(r, pos + offset))
}

// ---------------------------------------------------------------------------
// Actions

/// Action data shared by all transitions (`compileActions`): the packed data and
/// the index of each action sequence in it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ActionIndex {
	/// The packed actions.
	pub data: Vec<u8>,
	/// The index of each packed sequence of ligature actions, and of its suffixes.
	pub ligatures: HashMap<Vec<u8>, usize>,
	/// The index of each sequence of inserted glyphs, and of its subsequences.
	pub insertions: HashMap<Vec<u32>, usize>,
}

fn compile_lig_actions(actions: &[LigAction]) -> Vec<u8> {
	let mut out = Vec::with_capacity(actions.len() * 4);
	for (i, a) in actions.iter().enumerate() {
		let mut value = (a.glyph_index_delta & 0x3FFF_FFFF) as u32;
		if i + 1 == actions.len() {
			value |= 0x8000_0000;
		}
		if a.store {
			value |= 0x4000_0000;
		}
		out.extend_from_slice(&value.to_be_bytes());
	}
	out
}

/// Packs the actions of all transitions of `states` (`compileActions`); `None`
/// for the kinds of actions that have no separate action data.
pub fn compile_actions(
	kind: ActionKind,
	font: &mut Font,
	states: &[AatState],
) -> Result<Option<ActionIndex>> {
	let transitions = || states.iter().flat_map(|s| s.transitions.values());
	match kind {
		ActionKind::Ligature => {
			let actions: HashSet<Vec<u8>> = transitions()
				.map(|t| compile_lig_actions(&t.actions))
				.collect();
			let mut actions: Vec<Vec<u8>> = actions.into_iter().collect();
			// Longer sequences first: shorter ones may be their suffixes.
			actions.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
			let mut index = ActionIndex::default();
			for a in actions {
				if index.ligatures.contains_key(&a) {
					continue;
				}
				for i in (0..a.len()).step_by(4) {
					let at = (index.data.len() + i) / 4;
					index.ligatures.entry(a[i..].to_vec()).or_insert(at);
				}
				index.data.extend_from_slice(&a);
			}
			index.data = pad(&index.data, 4);
			Ok(Some(index))
		}
		ActionKind::Insertion => {
			let order = font.glyph_order()?.clone();
			let mut set: HashSet<Vec<u32>> = HashSet::new();
			for t in transitions() {
				set.insert(t.current_insertion_action.clone());
				set.insert(t.marked_insertion_action.clone());
			}
			// Sorted by decreasing length, then by glyph names.
			let mut actions: Vec<(Vec<String>, Vec<u32>)> = set
				.into_iter()
				.map(|a| (a.iter().map(|&g| order.name(g).into_owned()).collect(), a))
				.collect();
			actions.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
			let mut index = ActionIndex::default();
			for (_, action) in actions {
				if index.insertions.contains_key(&action) {
					continue;
				}
				for start in 0..action.len() {
					let at = index.data.len() / 2 + start;
					for limit in start..action.len() {
						index
							.insertions
							.entry(action[start..=limit].to_vec())
							.or_insert(at);
					}
				}
				for &g in &action {
					index
						.data
						.extend_from_slice(&ushort(g as i64)?.to_be_bytes());
				}
			}
			Ok(Some(index))
		}
		_ => Ok(None),
	}
}

/// `AATAction.compile`: the extended (`morx`) entry.
fn compile_action(t: &AatAction, index: Option<&ActionIndex>) -> Result<Vec<u8>> {
	let mut out = Vec::with_capacity(8);
	out.extend_from_slice(&ushort(t.new_state)?.to_be_bytes());
	let mut flags = t.reserved_flags;
	match t.kind {
		ActionKind::Rearrangement => {
			if !(0..=15).contains(&t.verb) {
				return Err(assertion(t.verb.to_string()));
			}
			flags |= t.verb;
			for (i, bit) in [0x8000, 0x4000, 0x2000].into_iter().enumerate() {
				if t.flag(i) {
					flags |= bit;
				}
			}
			out.extend_from_slice(&ushort(flags)?.to_be_bytes());
		}
		ActionKind::Contextual => {
			for (i, bit) in [0x8000, 0x4000].into_iter().enumerate() {
				if t.flag(i) {
					flags |= bit;
				}
			}
			out.extend_from_slice(&ushort(flags)?.to_be_bytes());
			out.extend_from_slice(&ushort(t.mark_index)?.to_be_bytes());
			out.extend_from_slice(&ushort(t.current_index)?.to_be_bytes());
		}
		ActionKind::Ligature => {
			let index = index.ok_or_else(|| assertion("actionIndex is not None"))?;
			for (i, bit) in [0x8000, 0x4000].into_iter().enumerate() {
				if t.flag(i) {
					flags |= bit;
				}
			}
			if !t.actions.is_empty() {
				flags |= 0x2000;
			}
			out.extend_from_slice(&ushort(flags)?.to_be_bytes());
			let at = if t.actions.is_empty() {
				0
			} else {
				let key = compile_lig_actions(&t.actions);
				*index
					.ligatures
					.get(&key)
					.ok_or_else(|| Error::ttlib("KeyError: ligature actions"))? as i64
			};
			out.extend_from_slice(&ushort(at)?.to_be_bytes());
		}
		ActionKind::Insertion => {
			let index = index.ok_or_else(|| assertion("actionIndex is not None"))?;
			for (i, bit) in [0x8000, 0x4000, 0x2000, 0x1000, 0x0800, 0x0400]
				.into_iter()
				.enumerate()
			{
				if t.flag(i) {
					flags |= bit;
				}
			}
			flags |= (t.current_insertion_action.len() as i64) << 5;
			flags |= t.marked_insertion_action.len() as i64;
			out.extend_from_slice(&ushort(flags)?.to_be_bytes());
			for action in [&t.current_insertion_action, &t.marked_insertion_action] {
				let at = if action.is_empty() {
					0xFFFF
				} else {
					*index
						.insertions
						.get(action)
						.ok_or_else(|| Error::ttlib("KeyError: insertion action"))? as i64
				};
				out.extend_from_slice(&ushort(at)?.to_be_bytes());
			}
		}
	}
	Ok(out)
}

fn decompile_lig_actions(action_reader: &OtReader<'_>, index: usize) -> Result<Vec<LigAction>> {
	let mut r = action_reader.sub_reader(action_reader.pos() + index * 4);
	let mut actions = Vec::new();
	loop {
		let value = r.read_u32()?;
		let mut delta = (value & 0x3FFF_FFFF) as i64;
		if delta >= 0x2000_0000 {
			delta -= 0x4000_0000;
		}
		actions.push(LigAction {
			store: value & 0x4000_0000 != 0,
			glyph_index_delta: delta,
		});
		if value & 0x8000_0000 != 0 {
			return Ok(actions);
		}
	}
}

fn decompile_insertion(action_reader: &OtReader<'_>, index: u16, count: u16) -> Result<Vec<u32>> {
	if index == 0xFFFF || count == 0 {
		return Ok(Vec::new());
	}
	let mut r = action_reader.sub_reader(action_reader.pos() + index as usize * 2);
	Ok(r.read_u16_array(count as usize)?
		.into_iter()
		.map(u32::from)
		.collect())
}

/// `AATAction.decompile`: an extended (`morx`) entry.
fn decompile_action(
	kind: ActionKind,
	r: &mut OtReader<'_>,
	action_reader: Option<&OtReader<'_>>,
) -> Result<AatAction> {
	let mut t = AatAction::new(kind);
	t.new_state = r.read_u16()? as i64;
	let flags = r.read_u16()? as i64;
	match kind {
		ActionKind::Rearrangement => {
			t.verb = flags & 0xF;
			t.flags = vec![
				flags & 0x8000 != 0,
				flags & 0x4000 != 0,
				flags & 0x2000 != 0,
			];
			t.reserved_flags = flags & 0x1FF0;
		}
		ActionKind::Contextual => {
			t.flags = vec![flags & 0x8000 != 0, flags & 0x4000 != 0];
			t.reserved_flags = flags & 0x3FFF;
			t.mark_index = r.read_u16()? as i64;
			t.current_index = r.read_u16()? as i64;
		}
		ActionKind::Ligature => {
			let action_reader =
				action_reader.ok_or_else(|| assertion("actionReader is not None"))?;
			t.flags = vec![flags & 0x8000 != 0, flags & 0x4000 != 0];
			t.reserved_flags = flags & 0x1FFF;
			let index = r.read_u16()? as usize;
			if flags & 0x2000 != 0 {
				t.actions = decompile_lig_actions(action_reader, index)?;
			}
		}
		ActionKind::Insertion => {
			let action_reader =
				action_reader.ok_or_else(|| assertion("actionReader is not None"))?;
			t.flags = [0x8000, 0x4000, 0x2000, 0x1000, 0x0800, 0x0400]
				.into_iter()
				.map(|b| flags & b != 0)
				.collect();
			let current = r.read_u16()?;
			t.current_insertion_action =
				decompile_insertion(action_reader, current, ((flags & 0x03E0) >> 5) as u16)?;
			let marked = r.read_u16()?;
			t.marked_insertion_action =
				decompile_insertion(action_reader, marked, (flags & 0x001F) as u16)?;
		}
	}
	Ok(t)
}

const VERBS: [&str; 16] = [
	"no change",
	"Ax ⇒ xA",
	"xD ⇒ Dx",
	"AxD ⇒ DxA",
	"ABx ⇒ xAB",
	"ABx ⇒ xBA",
	"xCD ⇒ CDx",
	"xCD ⇒ DCx",
	"AxCD ⇒ CDxA",
	"AxCD ⇒ DCxA",
	"ABxD ⇒ DxAB",
	"ABxD ⇒ DxBA",
	"ABxCD ⇒ CDxAB",
	"ABxCD ⇒ CDxBA",
	"ABxCD ⇒ DCxAB",
	"ABxCD ⇒ DCxBA",
];

/// `AATAction.toXML`.
fn action_to_xml(
	t: &AatAction,
	xw: &mut XmlWriter,
	font: &mut Font,
	attrs: &fontoxide_misc::xml_writer::Attrs<'_>,
	name: &str,
) -> Result<()> {
	xw.begintag(name, attrs);
	xw.newline();
	xw.simpletag("NewState", &[("value", &t.new_state)]);
	xw.newline();
	let names: Vec<&str> = t
		.kind
		.flag_names()
		.iter()
		.enumerate()
		.filter(|&(i, _)| t.flag(i))
		.map(|(_, n)| *n)
		.collect();
	if !names.is_empty() {
		xw.simpletag("Flags", &[("value", &names.join(","))]);
		xw.newline();
	}
	if t.reserved_flags != 0 {
		xw.simpletag(
			"ReservedFlags",
			&[("value", &format!("0x{:04X}", t.reserved_flags))],
		);
		xw.newline();
	}
	let order = font.glyph_order()?.clone();
	match t.kind {
		ActionKind::Rearrangement => {
			xw.simpletag("Verb", &[("value", &t.verb)]);
			if let Some(c) = usize::try_from(t.verb).ok().and_then(|v| VERBS.get(v)) {
				xw.comment(c);
			}
			xw.newline();
		}
		ActionKind::Contextual => {
			xw.simpletag("MarkIndex", &[("value", &t.mark_index)]);
			xw.newline();
			xw.simpletag("CurrentIndex", &[("value", &t.current_index)]);
			xw.newline();
		}
		ActionKind::Ligature => {
			for a in &t.actions {
				let delta = a.glyph_index_delta.to_string();
				let mut attrs: Vec<(&str, &dyn fontoxide_misc::xml_writer::AttrValue)> =
					vec![("GlyphIndexDelta", &delta)];
				if a.store {
					attrs.push(("Flags", &"Store"));
				}
				xw.simpletag_ordered("Action", &attrs);
				xw.newline();
			}
		}
		ActionKind::Insertion => {
			for (name, glyphs) in [
				("CurrentInsertionAction", &t.current_insertion_action),
				("MarkedInsertionAction", &t.marked_insertion_action),
			] {
				for &g in glyphs {
					xw.simpletag(name, &[("glyph", &order.name(g))]);
					xw.newline();
				}
			}
		}
	}
	xw.endtag(name);
	xw.newline();
	Ok(())
}

fn attr<'a>(e: &'a Element, name: &str) -> Result<&'a str> {
	e.attr(name)
		.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
}

fn eval(s: &str) -> Result<i64> {
	safe_eval_int(s)
		.and_then(|v| i64::try_from(v).ok())
		.ok_or_else(|| Error::value(format!("malformed node or string: {s:?}")))
}

/// `AATAction.fromXML`.
fn action_from_xml(kind: ActionKind, e: &Element, font: &mut Font) -> Result<AatAction> {
	let mut t = AatAction::new(kind);
	for c in e.elements() {
		match c.name.as_str() {
			"NewState" => t.new_state = eval(attr(c, "value")?)?,
			"Flags" => {
				for flag in attr(c, "value")?.split(',') {
					let flag = flag.trim();
					let i = kind
						.flag_names()
						.iter()
						.position(|n| *n == flag)
						.ok_or_else(|| assertion(format!("unsupported flag {flag}")))?;
					t.flags[i] = true;
				}
			}
			"ReservedFlags" if kind != ActionKind::Insertion => {
				t.reserved_flags = eval(attr(c, "value")?)?
			}
			"Verb" if kind == ActionKind::Rearrangement => t.verb = eval(attr(c, "value")?)?,
			"MarkIndex" if kind == ActionKind::Contextual => {
				t.mark_index = eval(attr(c, "value")?)?
			}
			"CurrentIndex" if kind == ActionKind::Contextual => {
				t.current_index = eval(attr(c, "value")?)?
			}
			"Action" if kind == ActionKind::Ligature => {
				let store = c
					.attr("Flags")
					.unwrap_or("")
					.split(',')
					.any(|f| f.trim() == "Store");
				t.actions.push(LigAction {
					store,
					glyph_index_delta: eval(attr(c, "GlyphIndexDelta")?)?,
				});
			}
			"CurrentInsertionAction" if kind == ActionKind::Insertion => {
				let g = font.glyph_id(attr(c, "glyph")?)?;
				t.current_insertion_action.push(g);
			}
			"MarkedInsertionAction" if kind == ActionKind::Insertion => {
				let g = font.glyph_id(attr(c, "glyph")?)?;
				t.marked_insertion_action.push(g);
			}
			other if kind == ActionKind::Insertion => return Err(assertion(other.to_owned())),
			_ => {}
		}
	}
	Ok(t)
}

// ---------------------------------------------------------------------------
// Extended state tables (`morx`)

/// `_countPerGlyphLookups`: the contextual actions do not say how many lookups
/// there are, so it is the highest index they use, plus one.
fn count_per_glyph_lookups(table: &AatStateTable) -> i64 {
	let mut n = 0;
	for state in &table.states {
		for t in state.transitions.values() {
			if t.kind != ActionKind::Contextual {
				continue;
			}
			if t.mark_index != 0xFFFF {
				n = n.max(t.mark_index + 1);
			}
			if t.current_index != 0xFFFF {
				n = n.max(t.current_index + 1);
			}
		}
	}
	n
}

fn read_stx(kind: ActionKind, r: &mut OtReader<'_>, font: &mut Font) -> Result<AatStateTable> {
	let mut table = AatStateTable::default();
	let pos = r.pos();
	table.glyph_class_count = r.read_u32()? as i64;
	let class_table = offset_reader(r, pos)?;
	let mut state_array = offset_reader(r, pos)?;
	let entry_table = offset_reader(r, pos)?;
	let per_glyph = if kind == ActionKind::Contextual {
		Some(offset_reader(r, pos)?)
	} else {
		None
	};
	let mut action_reader = None;
	match kind {
		ActionKind::Ligature => {
			action_reader = Some(offset_reader(r, pos)?);
			let mut components = offset_reader(r, pos)?;
			let mut ligatures = offset_reader(r, pos)?;
			let n = (ligatures.pos() as i64 - components.pos() as i64).div_euclid(2);
			if n < 0 {
				return Err(assertion("numLigComponents >= 0"));
			}
			table.lig_components = Some(
				components
					.read_u16_array(n as usize)?
					.into_iter()
					.map(i64::from)
					.collect(),
			);
			let limit = ligatures.data().len();
			let n = (limit as i64 - ligatures.pos() as i64).div_euclid(2).max(0);
			table.ligatures = Some(
				ligatures
					.read_u16_array(n as usize)?
					.into_iter()
					.map(u32::from)
					.collect(),
			);
		}
		ActionKind::Insertion => {
			action_reader = Some(offset_reader(r, pos)?);
		}
		_ => {}
	}
	let classes: AatLookup<u16> = read_aat_lookup(&mut class_table.clone(), font)?;
	table.glyph_classes = classes.into_iter().map(|(g, c)| (g, c as i64)).collect();
	if table.glyph_class_count == 0 {
		return Err(Error::ttlib("ZeroDivisionError: division by zero"));
	}
	// `int(a / b)`: truncated toward zero.
	let num_states = (entry_table.pos() as f64 - state_array.pos() as f64)
		/ (table.glyph_class_count * 2) as f64;
	let num_states = num_states as i64;
	let size = kind.static_size();
	for _ in 0..num_states {
		let mut state = AatState::default();
		for glyph_class in 0..table.glyph_class_count {
			let entry = state_array.read_u16()? as usize;
			let mut entry_reader = entry_table.sub_reader(entry_table.pos() + entry * size);
			let t = decompile_action(kind, &mut entry_reader, action_reader.as_ref())?;
			state.transitions.insert(glyph_class, t);
		}
		table.states.push(state);
	}
	if let Some(per_glyph) = per_glyph {
		let mut offsets = per_glyph.clone();
		let base = per_glyph.pos();
		for _ in 0..count_per_glyph_lookups(&table) {
			let offset = offsets.read_u32()? as usize;
			let mut lookup = reader_at(&per_glyph, base + offset);
			table
				.per_glyph_lookups
				.push(read_aat_lookup::<u32>(&mut lookup, font)?);
		}
	}
	Ok(table)
}

fn write_stx(
	kind: ActionKind,
	w: &mut OtWriter<'_>,
	font: &mut Font,
	table: &AatStateTable,
) -> Result<()> {
	let tag = w.table_tag();
	let classes: AatLookup<u16> = table
		.glyph_classes
		.iter()
		.map(|(&g, &c)| Ok((g, ushort(c)?)))
		.collect::<Result<_>>()?;
	let class_data = {
		let mut graph = WriterGraph::new(tag);
		write_aat_lookup(&mut graph.root(), font, &classes)?;
		pad(&graph.get_all_data()?, 2)
	};
	let class_count = table
		.glyph_classes
		.values()
		.max()
		.map(|m| m + 1)
		.ok_or_else(|| Error::value("max() arg is an empty sequence"))?;
	let mut class_table_offset = 16;
	if kind == ActionKind::Contextual {
		class_table_offset += 4;
	}
	class_table_offset += kind.action_header_size();
	let actions = compile_actions(kind, font, &table.states)?;
	let (state_array, entry_table) = compile_states(table, class_count, actions.as_ref())?;
	let state_array_offset = class_table_offset + class_data.len();
	let entry_table_offset = state_array_offset + state_array.len();
	let per_glyph_offset = entry_table_offset + entry_table.len();
	let per_glyph_data = pad(&compile_per_glyph_lookups(kind, table, font, tag)?, 4);
	let action_offset = actions
		.as_ref()
		.map(|_| entry_table_offset + entry_table.len());
	let lig_components: Option<Vec<u8>> = match &table.lig_components {
		Some(c) => Some(
			c.iter()
				.map(|&v| ushort(v).map(u16::to_be_bytes))
				.collect::<Result<Vec<_>>>()?
				.concat(),
		),
		None => None,
	};
	let ligatures: Option<Vec<u8>> = match &table.ligatures {
		Some(l) => Some(
			l.iter()
				.map(|&g| ushort(g as i64).map(u16::to_be_bytes))
				.collect::<Result<Vec<_>>>()?
				.concat(),
		),
		None => None,
	};
	let mut lig_offsets = None;
	if let Some(c) = &lig_components {
		if !per_glyph_data.is_empty() {
			return Err(assertion("len(perGlyphData) == 0"));
		}
		let action_offset = action_offset.ok_or_else(|| {
			Error::ttlib("TypeError: unsupported operand type(s) for +: 'NoneType' and 'int'")
		})?;
		let action_len = actions.as_ref().map_or(0, |a| a.data.len());
		let components = action_offset + action_len;
		lig_offsets = Some((components, components + c.len()));
	}
	w.write_u32(ulong(class_count)?);
	for v in [class_table_offset, state_array_offset, entry_table_offset] {
		w.write_u32(ulong(v as i64)?);
	}
	if kind == ActionKind::Contextual {
		w.write_u32(ulong(per_glyph_offset as i64)?);
	}
	if let Some(o) = action_offset {
		w.write_u32(ulong(o as i64)?);
	}
	if let Some((components, ligatures)) = lig_offsets {
		w.write_u32(ulong(components as i64)?);
		w.write_u32(ulong(ligatures as i64)?);
	}
	w.write_data(&class_data);
	w.write_data(&state_array);
	w.write_data(&entry_table);
	w.write_data(&per_glyph_data);
	if let Some(a) = &actions {
		w.write_data(&a.data);
	}
	if let Some(c) = &lig_components {
		w.write_data(c);
	}
	if let Some(l) = &ligatures {
		w.write_data(l);
	}
	Ok(())
}

fn transition(state: &AatState, glyph_class: i64) -> Result<&AatAction> {
	state
		.transitions
		.get(&glyph_class)
		.ok_or_else(|| Error::ttlib(format!("KeyError: {glyph_class}")))
}

/// `_compileStates`: the state array and the shared entries.
fn compile_states(
	table: &AatStateTable,
	class_count: i64,
	actions: Option<&ActionIndex>,
) -> Result<(Vec<u8>, Vec<u8>)> {
	let mut entries: Vec<Vec<u8>> = Vec::new();
	let mut ids: HashMap<Vec<u8>, usize> = HashMap::new();
	let mut state_array = Vec::new();
	for state in &table.states {
		for glyph_class in 0..class_count {
			let t = transition(state, glyph_class)?;
			let data = compile_action(t, actions)?;
			if data.len() != t.kind.static_size() {
				return Err(assertion("staticSize mismatch"));
			}
			let id = match ids.get(&data) {
				Some(&id) => id,
				None => {
					entries.push(data.clone());
					ids.insert(data, entries.len() - 1);
					entries.len() - 1
				}
			};
			state_array.extend_from_slice(&ushort(id as i64)?.to_be_bytes());
		}
	}
	Ok((pad(&state_array, 4), pad(&entries.concat(), 4)))
}

fn compile_per_glyph_lookups(
	kind: ActionKind,
	table: &AatStateTable,
	font: &mut Font,
	tag: fontoxide_misc::Tag,
) -> Result<Vec<u8>> {
	if kind != ActionKind::Contextual {
		return Ok(Vec::new());
	}
	let n = count_per_glyph_lookups(table);
	if table.per_glyph_lookups.len() as i64 != n {
		return Err(assertion(format!(
			"len(AATStateTable.PerGlyphLookups) is {}, but the actions inside the table refer to {n}",
			table.per_glyph_lookups.len()
		)));
	}
	let mut graph = WriterGraph::new(tag);
	{
		let mut w = graph.root();
		for lookup in &table.per_glyph_lookups {
			let mut sub = w.sub_table("PerGlyphLookup", None, 4);
			write_aat_lookup(&mut sub, font, lookup)?;
		}
	}
	graph.get_all_data()
}

// ---------------------------------------------------------------------------
// Classic state tables (`mort`)

fn mort_error(msg: &str) -> Error {
	Error::value(msg.to_owned())
}

fn read_legacy(kind: ActionKind, r: &mut OtReader<'_>, font: &mut Font) -> Result<AatStateTable> {
	let mut table = AatStateTable::default();
	let pos = r.pos();
	table.glyph_class_count = r.read_u16()? as i64;
	let class_table_offset = r.read_u16()? as usize;
	let state_array_offset = r.read_u16()? as i64;
	let entry_table_offset = r.read_u16()? as i64;
	if table.glyph_class_count == 0 {
		return Err(mort_error("mort state table has no glyph classes"));
	}
	let mut substitution_offset = None;
	let (mut component_offset, mut ligature_offset) = (None, None);
	let mut action_reader = None;
	match kind {
		ActionKind::Contextual => substitution_offset = Some(r.read_u16()? as i64),
		ActionKind::Ligature => {
			let _action_offset = r.read_u16()?;
			let components = r.read_u16()? as i64;
			let ligatures = r.read_u16()? as i64;
			if components > ligatures || (ligatures - components) % 2 != 0 {
				return Err(mort_error("invalid mort ligature component table"));
			}
			let raw = reader_at(r, pos + components as usize)
				.read_u16_array(((ligatures - components) / 2) as usize)?;
			if raw.iter().any(|c| c % 2 != 0) {
				return Err(mort_error("invalid mort ligature component value"));
			}
			table.lig_components = Some(raw.into_iter().map(|c| c as i64 / 2).collect());
			table.ligatures = Some(Vec::new());
			table.mort_ligature_rebase = Some(BTreeSet::new());
			component_offset = Some(components);
			ligature_offset = Some(ligatures);
		}
		ActionKind::Insertion => {
			let action_offset = r.read_u16()? as usize;
			action_reader = Some(reader_at(r, pos + action_offset));
		}
		ActionKind::Rearrangement => {}
	}
	let mut class_table = reader_at(r, pos + class_table_offset);
	let first_glyph = class_table.read_u16()? as u32;
	let glyph_count = class_table.read_u16()? as usize;
	let classes = class_table.read_u8_array(glyph_count)?;
	for (i, c) in classes.into_iter().enumerate() {
		table.glyph_classes.insert(first_glyph + i as u32, c as i64);
	}
	let mut state_array = reader_at(r, pos + state_array_offset as usize);
	let entry_table = reader_at(r, pos + entry_table_offset.max(0) as usize);
	let state_data_length = entry_table_offset - state_array_offset;
	if state_data_length < 0 {
		return Err(mort_error(
			"mort state-array offset follows its entry table",
		));
	}
	let num_states = state_data_length / table.glyph_class_count;
	let glyph_count = font.glyph_order()?.len() as i64;
	let entry_size = if kind == ActionKind::Ligature {
		4
	} else {
		kind.static_size()
	};
	// Entries are read once each, as fontTools caches them by index.
	let mut cache: HashMap<u8, (AatAction, i64, i64)> = HashMap::new();
	// The entry of each glyph class in each state.
	let mut state_entries: Vec<Vec<u8>> = Vec::new();
	for _ in 0..num_states {
		let mut row = Vec::new();
		for _ in 0..table.glyph_class_count {
			let entry = state_array.read_u8()?;
			row.push(entry);
			if let std::collections::hash_map::Entry::Vacant(slot) = cache.entry(entry) {
				let mut er =
					entry_table.sub_reader(entry_table.pos() + entry as usize * entry_size);
				let mut offsets = (0, 0);
				let mut t = match kind {
					ActionKind::Contextual => {
						let mut t = AatAction::new(kind);
						t.new_state = er.read_u16()? as i64;
						let flags = er.read_u16()? as i64;
						t.flags = vec![flags & 0x8000 != 0, flags & 0x4000 != 0];
						t.reserved_flags = flags & 0x3FFF;
						offsets = (er.read_i16()? as i64, er.read_i16()? as i64);
						t
					}
					ActionKind::Ligature => {
						let mut t = AatAction::new(kind);
						t.new_state = er.read_u16()? as i64;
						let flags = er.read_u16()? as i64;
						t.flags = vec![flags & 0x8000 != 0, flags & 0x4000 != 0];
						let offset = flags & 0x3FFF;
						if offset != 0 {
							t.actions = read_legacy_lig_actions(
								r,
								&mut table,
								pos,
								offset as usize,
								component_offset.unwrap_or(0),
								glyph_count,
							)?;
						}
						t
					}
					_ => decompile_action(kind, &mut er, action_reader.as_ref())?,
				};
				let offset = t.new_state - state_array_offset;
				if offset < 0 || offset % table.glyph_class_count != 0 {
					return Err(mort_error("invalid mort new-state offset"));
				}
				t.new_state = offset / table.glyph_class_count;
				// For contextual entries: the mark and current offsets.
				slot.insert((t, offsets.0, offsets.1));
			}
		}
		state_entries.push(row);
	}
	if let Some(substitution_offset) = substitution_offset {
		read_legacy_contextual_lookups(&mut table, r, font, pos, substitution_offset, &mut cache)?;
	}
	for row in &state_entries {
		let mut state = AatState::default();
		for (glyph_class, entry) in row.iter().enumerate() {
			state
				.transitions
				.insert(glyph_class as i64, cache[entry].0.clone());
		}
		table.states.push(state);
	}
	if substitution_offset.is_none()
		&& let Some(ligature_offset) = ligature_offset
	{
		let base = ligature_offset / 2;
		let components = table.lig_components.get_or_insert_with(Vec::new);
		let rebase = table.mort_ligature_rebase.get_or_insert_with(BTreeSet::new);
		rebase.retain(|&i| components[i as usize] >= base);
		for &i in rebase.iter() {
			components[i as usize] -= base;
		}
		let count = count_legacy_ligatures(&table, glyph_count);
		let mut lr = reader_at(r, pos + ligature_offset as usize);
		table.ligatures = Some(
			lr.read_u16_array(count as usize)?
				.into_iter()
				.map(u32::from)
				.collect(),
		);
	}
	Ok(table)
}

fn read_legacy_lig_actions(
	r: &OtReader<'_>,
	table: &mut AatStateTable,
	pos: usize,
	offset: usize,
	component_offset: i64,
	glyph_count: i64,
) -> Result<Vec<LigAction>> {
	let mut ar = reader_at(r, pos + offset);
	let n = table.lig_components.as_ref().map_or(0, Vec::len) as i64;
	let rebase = table.mort_ligature_rebase.get_or_insert_with(BTreeSet::new);
	let mut result = Vec::new();
	let mut rebased = false;
	loop {
		let value = ar.read_u32()?;
		let last = value & 0x8000_0000 != 0;
		let store = value & 0x4000_0000 != 0;
		let mut word = (value & 0x3FFF_FFFF) as i64;
		if word & 0x2000_0000 != 0 {
			word -= 0x4000_0000;
		}
		let index = word - component_offset.div_euclid(2);
		if index >= n || index + glyph_count <= 0 {
			return Err(mort_error(
				"mort ligature component offset is out of bounds",
			));
		}
		if (store || last) && !rebased {
			rebase.extend(index.max(0)..n.min(index + glyph_count));
			rebased = true;
		}
		result.push(LigAction {
			store,
			glyph_index_delta: index,
		});
		if last {
			return Ok(result);
		}
	}
}

/// `_countLegacyLigatures`: the legacy format does not store the number of
/// ligatures, so it is the highest index the actions can reach, plus one.
fn count_legacy_ligatures(table: &AatStateTable, glyph_count: i64) -> i64 {
	let components = table.lig_components.as_deref().unwrap_or_default();
	let n = components.len() as i64;
	let mut count = 0;
	for t in table.states.iter().flat_map(|s| s.transitions.values()) {
		let mut cumulative = 0;
		for (i, a) in t.actions.iter().enumerate() {
			let start = a.glyph_index_delta.max(0);
			let limit = n.min(a.glyph_index_delta + glyph_count);
			let max = if start < limit {
				components[start as usize..limit as usize]
					.iter()
					.copied()
					.max()
					.unwrap_or(0)
			} else {
				0
			};
			cumulative += max;
			if a.store || i + 1 == t.actions.len() {
				count = count.max(cumulative + 1);
			}
		}
	}
	count
}

fn read_legacy_contextual_lookups(
	table: &mut AatStateTable,
	r: &OtReader<'_>,
	font: &mut Font,
	pos: usize,
	substitution_offset: i64,
	cache: &mut HashMap<u8, (AatAction, i64, i64)>,
) -> Result<()> {
	let offsets: BTreeSet<i64> = cache
		.values()
		.flat_map(|&(_, m, c)| [m, c])
		.filter(|&o| o != 0)
		.collect();
	let index: HashMap<i64, i64> = offsets
		.iter()
		.enumerate()
		.map(|(i, &o)| (o, i as i64))
		.collect();
	let order = font.glyph_order()?.clone();
	let num_glyphs = order.len() as i64;
	let available = r.data().len() as i64 - pos as i64;
	for &offset in &offsets {
		// The offset plus a glyph ID is the word offset of its replacement;
		// words before the substitution table and past the subtable are left out.
		let first = 0.max((substitution_offset + 1).div_euclid(2) - offset);
		let last = (num_glyphs - 1).min((available - 2).div_euclid(2) - offset);
		let mut lookup = BTreeMap::new();
		if first <= last {
			let mut lr = reader_at(r, (pos as i64 + 2 * (offset + first)) as usize);
			let replacements = lr.read_u16_array((last - first + 1) as usize)?;
			for (i, rep) in replacements.into_iter().enumerate() {
				if rep != 0 {
					// Keyed by name in fontTools: the last of duplicate names wins.
					let gid = (first + i as i64) as u32;
					let gid = order.id(&order.name(gid)).unwrap_or(gid);
					lookup.insert(gid, rep as u32);
				}
			}
		}
		table.per_glyph_lookups.push(lookup);
	}
	let resolve = |o: i64| if o == 0 { 0xFFFF } else { index[&o] };
	for (t, m, c) in cache.values_mut() {
		t.mark_index = resolve(*m);
		t.current_index = resolve(*c);
	}
	Ok(())
}

fn aligned(value: usize, alignment: usize) -> usize {
	value.div_ceil(alignment) * alignment
}

/// A ligature action sequence, as `mort` keys them.
type LigSequence = Vec<(i64, bool)>;

/// The extra header, the data after the entries, and the offset of each
/// ligature action sequence.
type LegacyLigatureData = (Vec<u8>, Vec<u8>, HashMap<LigSequence, usize>);

fn lig_sequence(t: &AatAction) -> LigSequence {
	t.actions
		.iter()
		.map(|a| (a.glyph_index_delta, a.store))
		.collect()
}

fn write_legacy(
	kind: ActionKind,
	w: &mut OtWriter<'_>,
	font: &mut Font,
	table: &AatStateTable,
) -> Result<()> {
	let count = table.glyph_class_count;
	if !(1..=256).contains(&count) {
		return Err(mort_error(
			"mort state tables support 1 to 256 glyph classes",
		));
	}
	let (first_glyph, classes) = match (
		table.glyph_classes.keys().next(),
		table.glyph_classes.keys().next_back(),
	) {
		(Some(&first), Some(&last)) => {
			let classes = (first..=last)
				.map(|g| {
					let c = table.glyph_classes.get(&g).copied().unwrap_or(1);
					u8::try_from(c).map_err(|_| mort_error("bytes must be in range(0, 256)"))
				})
				.collect::<Result<Vec<u8>>>()?;
			(first, classes)
		}
		_ => (0, Vec::new()),
	};
	let mut class_data = Vec::new();
	class_data.extend_from_slice(&ushort(first_glyph as i64)?.to_be_bytes());
	class_data.extend_from_slice(&ushort(classes.len() as i64)?.to_be_bytes());
	class_data.extend_from_slice(&classes);

	let generic = compile_actions(kind, font, &table.states)?;
	let mut entries: Vec<&AatAction> = Vec::new();
	let mut ids: HashMap<Vec<u8>, usize> = HashMap::new();
	let mut state_data = Vec::new();
	for state in &table.states {
		for glyph_class in 0..count {
			let t = transition(state, glyph_class)?;
			let key = compile_action(t, generic.as_ref())?;
			let id = match ids.get(&key) {
				Some(&id) => id,
				None => {
					let id = entries.len();
					if id > 0xFF {
						return Err(mort_error("mort state tables support at most 256 entries"));
					}
					ids.insert(key, id);
					entries.push(t);
					id
				}
			};
			state_data.push(id as u8);
		}
	}
	let extra_size = match kind {
		ActionKind::Contextual | ActionKind::Insertion => 2,
		ActionKind::Ligature => 6,
		ActionKind::Rearrangement => 0,
	};
	let class_table_offset = 8 + extra_size;
	let state_array_offset = aligned(class_table_offset + class_data.len(), 2);
	let entry_table_offset = aligned(state_array_offset + state_data.len(), 2);
	let entry_size = if kind == ActionKind::Ligature {
		4
	} else {
		kind.static_size()
	};
	let tail_offset = entry_table_offset + entries.len() * entry_size;

	let mut extra = Vec::new();
	let mut tail = Vec::new();
	let mut context_offsets = Vec::new();
	let mut lig_offsets: HashMap<LigSequence, usize> = HashMap::new();
	match kind {
		ActionKind::Contextual => {
			let n = count_per_glyph_lookups(table);
			if table.per_glyph_lookups.len() as i64 != n {
				return Err(mort_error(&format!(
					"mort contextual actions refer to {n} lookups, but the table has {}",
					table.per_glyph_lookups.len()
				)));
			}
			let num_glyphs = font.glyph_order()?.len() as u32;
			for lookup in &table.per_glyph_lookups {
				let offset = (tail_offset + tail.len()) / 2;
				if offset > 0x7FFF {
					return Err(mort_error(
						"mort substitution offset exceeds signed 16 bits",
					));
				}
				context_offsets.push(offset as i64);
				for gid in 0..num_glyphs {
					let rep = lookup.get(&gid).copied().unwrap_or(0);
					tail.extend_from_slice(&ushort(rep as i64)?.to_be_bytes());
				}
			}
			if tail_offset > 0xFFFF {
				return Err(mort_error("mort substitution-table offset exceeds 16 bits"));
			}
			extra.extend_from_slice(&(tail_offset as u16).to_be_bytes());
		}
		ActionKind::Ligature => {
			let (e, t, o) = compile_legacy_ligature_data(table, font, &entries, tail_offset)?;
			extra = e;
			tail = t;
			lig_offsets = o;
		}
		ActionKind::Insertion => {
			if tail_offset > 0xFFFF {
				return Err(mort_error("mort insertion-action offset exceeds 16 bits"));
			}
			extra.extend_from_slice(&(tail_offset as u16).to_be_bytes());
			tail = generic.as_ref().map(|g| g.data.clone()).unwrap_or_default();
		}
		ActionKind::Rearrangement => {}
	}
	let mut entry_data = Vec::new();
	for t in &entries {
		let new_state = state_array_offset as i64 + t.new_state * count;
		if !(0..=0xFFFF).contains(&new_state) {
			return Err(mort_error("mort new-state offset exceeds 16 bits"));
		}
		match kind {
			ActionKind::Contextual => {
				let mut flags = t.reserved_flags;
				if t.flag(0) {
					flags |= 0x8000;
				}
				if t.flag(1) {
					flags |= 0x4000;
				}
				let offset = |i: i64| -> Result<i64> {
					if i == 0xFFFF {
						return Ok(0);
					}
					usize::try_from(i)
						.ok()
						.and_then(|i| context_offsets.get(i).copied())
						.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))
				};
				let short = |v: i64| {
					i16::try_from(v).map_err(|_| {
						Error::Struct("'h' format requires -32768 <= number <= 32767".into())
					})
				};
				entry_data.extend_from_slice(&(new_state as u16).to_be_bytes());
				entry_data.extend_from_slice(&ushort(flags)?.to_be_bytes());
				entry_data.extend_from_slice(&short(offset(t.mark_index)?)?.to_be_bytes());
				entry_data.extend_from_slice(&short(offset(t.current_index)?)?.to_be_bytes());
			}
			ActionKind::Ligature => {
				if t.reserved_flags != 0 {
					return Err(mort_error(
						"mort ligature entries cannot encode reserved flags",
					));
				}
				let mut flags = 0;
				if t.flag(0) {
					flags |= 0x8000;
				}
				if t.flag(1) {
					flags |= 0x4000;
				}
				flags |= lig_offsets.get(&lig_sequence(t)).copied().unwrap_or(0);
				entry_data.extend_from_slice(&(new_state as u16).to_be_bytes());
				entry_data.extend_from_slice(&ushort(flags as i64)?.to_be_bytes());
			}
			_ => {
				let data = compile_action(t, generic.as_ref())?;
				entry_data.extend_from_slice(&(new_state as u16).to_be_bytes());
				entry_data.extend_from_slice(&data[2..]);
			}
		}
	}
	let mut data = Vec::new();
	for v in [
		count as usize,
		class_table_offset,
		state_array_offset,
		entry_table_offset,
	] {
		data.extend_from_slice(&ushort(v as i64)?.to_be_bytes());
	}
	data.extend_from_slice(&extra);
	data.extend_from_slice(&class_data);
	let mut data = pad(&data, 2);
	data.extend_from_slice(&state_data);
	let mut data = pad(&data, 2);
	data.extend_from_slice(&entry_data);
	data.extend_from_slice(&tail);
	let data = pad(&data, 4);
	if data.len() > 0xFFFF {
		return Err(mort_error("mort state table exceeds 65535 bytes"));
	}
	w.write_data(&data);
	Ok(())
}

/// `_compileLigatureData`: the extra header, the action, component and
/// ligature data, and the offset of each action sequence.
fn compile_legacy_ligature_data(
	table: &AatStateTable,
	font: &mut Font,
	entries: &[&AatAction],
	tail_offset: usize,
) -> Result<LegacyLigatureData> {
	let mut sequences: Vec<LigSequence> = Vec::new();
	for t in entries {
		let s = lig_sequence(t);
		if !s.is_empty() && !sequences.contains(&s) {
			sequences.push(s);
		}
	}
	let components = table.lig_components.as_deref().ok_or_else(|| {
		Error::ttlib("AttributeError: 'AATStateTable' object has no attribute 'LigComponents'")
	})?;
	let action_offset = aligned(tail_offset, 4);
	let action_count: usize = sequences.iter().map(Vec::len).sum();
	let component_offset = action_offset + action_count * 4;
	let glyph_count = font.glyph_order()?.len() as i64;
	let ligature_offset = component_offset + components.len() * 2;
	if action_offset.max(component_offset).max(ligature_offset) > 0xFFFF {
		return Err(mort_error("mort ligature-table offset exceeds 16 bits"));
	}
	let mut offsets = HashMap::new();
	let mut action_data = Vec::new();
	let mut rebase: BTreeSet<i64> = table.mort_ligature_rebase.clone().unwrap_or_default();
	let n = components.len() as i64;
	for sequence in &sequences {
		let at = action_offset + action_data.len();
		if at > 0x3FFF {
			return Err(mort_error("mort ligature-action offset exceeds 14 bits"));
		}
		offsets.insert(sequence.clone(), at);
		let mut rebased = false;
		for (i, &(delta, store)) in sequence.iter().enumerate() {
			let value = (component_offset / 2) as i64 + delta;
			if !(-(1 << 29)..(1 << 29)).contains(&value) {
				return Err(mort_error("mort ligature component offset exceeds 30 bits"));
			}
			let last = i + 1 == sequence.len();
			let mut v = (value & 0x3FFF_FFFF) as u32;
			if store {
				v |= 0x4000_0000;
			}
			if last {
				v |= 0x8000_0000;
			}
			action_data.extend_from_slice(&v.to_be_bytes());
			if (store || last) && !rebased && table.mort_ligature_rebase.is_none() {
				rebase.extend(delta.max(0)..n.min(delta + glyph_count));
			}
			rebased |= store || last;
		}
	}
	let mut component_data = Vec::new();
	for (i, &c) in components.iter().enumerate() {
		let mut c = c * 2;
		if rebase.contains(&(i as i64)) {
			c += ligature_offset as i64;
		}
		if !(0..=0xFFFF).contains(&c) {
			return Err(mort_error("mort ligature component value exceeds 16 bits"));
		}
		component_data.extend_from_slice(&(c as u16).to_be_bytes());
	}
	let ligatures = table.ligatures.as_deref().ok_or_else(|| {
		Error::ttlib("AttributeError: 'AATStateTable' object has no attribute 'Ligatures'")
	})?;
	let mut ligature_data = Vec::new();
	for &g in ligatures {
		ligature_data.extend_from_slice(&ushort(g as i64)?.to_be_bytes());
	}
	let mut extra = Vec::new();
	for v in [action_offset, component_offset, ligature_offset] {
		extra.extend_from_slice(&(v as u16).to_be_bytes());
	}
	let mut tail = vec![0; action_offset - tail_offset];
	tail.extend(action_data);
	tail.extend(component_data);
	tail.extend(ligature_data);
	Ok((extra, tail, offsets))
}

// ---------------------------------------------------------------------------
// Converter entry points

const MORT: fontoxide_misc::Tag = fontoxide_misc::Tag::new(b"mort");

/// Reads a state table in the encoding of the table being read
/// (`MorphStateTable.read`).
pub fn read_morph_state_table(
	kind: ActionKind,
	r: &mut OtReader<'_>,
	font: &mut Font,
) -> Result<AatStateTable> {
	if r.table_tag == MORT {
		read_legacy(kind, r, font)
	} else {
		read_stx(kind, r, font)
	}
}

/// Writes a state table in the encoding of the table being written
/// (`MorphStateTable.write`).
pub fn write_morph_state_table(
	kind: ActionKind,
	w: &mut OtWriter<'_>,
	font: &mut Font,
	table: &AatStateTable,
) -> Result<()> {
	if w.table_tag() == MORT {
		write_legacy(kind, w, font, table)
	} else {
		write_stx(kind, w, font, table)
	}
}

/// Writes a state table as TTX (`STXHeader.xmlWrite`, for both encodings).
pub fn xml_morph_state_table(
	xw: &mut XmlWriter,
	font: &mut Font,
	name: &str,
	attrs: &[(&'static str, String)],
	table: &AatStateTable,
) -> Result<()> {
	super::table::begintag(xw, name, attrs);
	xw.newline();
	xw.comment(&format!("GlyphClassCount={}", table.glyph_class_count));
	xw.newline();
	let order = font.glyph_order()?.clone();
	let mut classes: Vec<(String, i64)> = table
		.glyph_classes
		.iter()
		.map(|(&g, &c)| (order.name(g).into_owned(), c))
		.collect();
	classes.sort();
	for (glyph, class) in classes {
		xw.simpletag("GlyphClass", &[("glyph", &glyph), ("value", &class)]);
		xw.newline();
	}
	for (i, state) in table.states.iter().enumerate() {
		xw.begintag("State", &[("index", &i)]);
		xw.newline();
		for (&glyph_class, t) in &state.transitions {
			action_to_xml(t, xw, font, &[("onGlyphClass", &glyph_class)], "Transition")?;
		}
		xw.endtag("State");
		xw.newline();
	}
	for (i, lookup) in table.per_glyph_lookups.iter().enumerate() {
		xw.begintag("PerGlyphLookup", &[("index", &i)]);
		xw.newline();
		let mut items: Vec<(String, String)> = lookup
			.iter()
			.map(|(&g, &v)| (order.name(g).into_owned(), order.name(v).into_owned()))
			.collect();
		items.sort();
		for (glyph, value) in items {
			xw.simpletag("Lookup", &[("glyph", &glyph), ("value", &value)]);
			xw.newline();
		}
		xw.endtag("PerGlyphLookup");
		xw.newline();
	}
	if let Some(components) = &table.lig_components {
		xw.begintag("LigComponents", &[]);
		xw.newline();
		for (i, v) in components.iter().enumerate() {
			xw.simpletag("LigComponent", &[("index", &i), ("value", v)]);
			xw.newline();
		}
		xw.endtag("LigComponents");
		xw.newline();
	}
	if let Some(rebase) = &table.mort_ligature_rebase {
		xw.begintag("MortLigatureRebase", &[]);
		xw.newline();
		for i in rebase {
			xw.simpletag("Component", &[("index", i)]);
			xw.newline();
		}
		xw.endtag("MortLigatureRebase");
		xw.newline();
	}
	if let Some(ligatures) = &table.ligatures {
		xw.begintag("Ligatures", &[]);
		xw.newline();
		for (i, &g) in ligatures.iter().enumerate() {
			xw.simpletag("Ligature", &[("index", &i), ("glyph", &order.name(g))]);
			xw.newline();
		}
		xw.endtag("Ligatures");
		xw.newline();
	}
	xw.endtag(name);
	xw.newline();
	Ok(())
}

/// Reads a state table from TTX (`STXHeader.xmlRead`).
pub fn read_morph_state_table_xml(
	kind: ActionKind,
	element: &Element,
	font: &mut Font,
) -> Result<AatStateTable> {
	let mut table = AatStateTable::default();
	for e in element.elements() {
		match e.name.as_str() {
			"GlyphClass" => {
				let glyph = font.glyph_id(attr(e, "glyph")?)?;
				table.glyph_classes.insert(glyph, eval(attr(e, "value")?)?);
			}
			"State" => {
				let mut state = AatState::default();
				for t in e.elements().filter(|t| t.name == "Transition") {
					let glyph_class = eval(attr(t, "onGlyphClass")?)?;
					state
						.transitions
						.insert(glyph_class, action_from_xml(kind, t, font)?);
				}
				table.states.push(state);
			}
			"PerGlyphLookup" => {
				if kind != ActionKind::Contextual {
					return Err(Error::ttlib(
						"AttributeError: 'NoneType' object has no attribute 'xmlRead'",
					));
				}
				let mut ctx = super::table::XmlCtx::default();
				table
					.per_glyph_lookups
					.push(read_aat_lookup_xml::<u32>(e, font, &mut ctx)?);
			}
			"LigComponents" => {
				table.lig_components = Some(
					e.elements()
						.filter(|c| c.name == "LigComponent")
						.map(|c| eval(attr(c, "value")?))
						.collect::<Result<_>>()?,
				);
			}
			"MortLigatureRebase" => {
				table.mort_ligature_rebase = Some(
					e.elements()
						.filter(|c| c.name == "Component")
						.map(|c| eval(attr(c, "index")?))
						.collect::<Result<_>>()?,
				);
			}
			"Ligatures" => {
				let mut ligatures = Vec::new();
				for c in e.elements().filter(|c| c.name == "Ligature") {
					ligatures.push(font.glyph_id(attr(c, "glyph")?)?);
				}
				table.ligatures = Some(ligatures);
			}
			_ => {}
		}
	}
	let max = table
		.glyph_classes
		.values()
		.copied()
		.chain(
			table
				.states
				.iter()
				.flat_map(|s| s.transitions.keys().copied()),
		)
		.max()
		.unwrap_or(-1);
	table.glyph_class_count = max + 1;
	Ok(table)
}

// ---------------------------------------------------------------------------
// Subtables of `mort` and `morx`

use crate::generated::ot::{
	ContextualMorph, InsertionMorph, LigatureMorph, NoncontextualMorph, RearrangementMorph,
};

/// The body of a morph subtable (`SubStruct`), by morph type.
#[derive(Debug, Clone, PartialEq)]
pub enum MorphSubStruct {
	/// Type 0.
	Rearrangement(RearrangementMorph),
	/// Type 1.
	Contextual(ContextualMorph),
	/// Type 2.
	Ligature(LigatureMorph),
	/// Type 4.
	Noncontextual(NoncontextualMorph),
	/// Type 5.
	Insertion(InsertionMorph),
}

impl MorphSubStruct {
	/// The morph type (`LookupType`).
	pub fn lookup_type(&self) -> u32 {
		match self {
			MorphSubStruct::Rearrangement(_) => 0,
			MorphSubStruct::Contextual(_) => 1,
			MorphSubStruct::Ligature(_) => 2,
			MorphSubStruct::Noncontextual(_) => 4,
			MorphSubStruct::Insertion(_) => 5,
		}
	}

	fn decompile(
		morph_type: u8,
		table: &str,
		r: &mut OtReader<'_>,
		font: &mut Font,
	) -> Result<Self> {
		Ok(match morph_type {
			0 => MorphSubStruct::Rearrangement(OtTable::decompile(r, font)?),
			1 => MorphSubStruct::Contextual(OtTable::decompile(r, font)?),
			2 => MorphSubStruct::Ligature(OtTable::decompile(r, font)?),
			4 => MorphSubStruct::Noncontextual(OtTable::decompile(r, font)?),
			5 => MorphSubStruct::Insertion(OtTable::decompile(r, font)?),
			t => return Err(assertion(format!("unsupported '{table}' lookup type {t}"))),
		})
	}

	fn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {
		match self {
			MorphSubStruct::Rearrangement(v) => v.compile(w, font),
			MorphSubStruct::Contextual(v) => v.compile(w, font),
			MorphSubStruct::Ligature(v) => v.compile(w, font),
			MorphSubStruct::Noncontextual(v) => v.compile(w, font),
			MorphSubStruct::Insertion(v) => v.compile(w, font),
		}
	}

	fn to_xml(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		match self {
			MorphSubStruct::Rearrangement(v) => v.to_xml(xw, font, &[], "RearrangementMorph"),
			MorphSubStruct::Contextual(v) => v.to_xml(xw, font, &[], "ContextualMorph"),
			MorphSubStruct::Ligature(v) => v.to_xml(xw, font, &[], "LigatureMorph"),
			MorphSubStruct::Noncontextual(v) => v.to_xml(xw, font, &[], "NoncontextualMorph"),
			MorphSubStruct::Insertion(v) => v.to_xml(xw, font, &[], "InsertionMorph"),
		}
	}

	/// Reads an element whose name ends with `Morph`; others fail with KeyError,
	/// as `getConverterByName` does.
	fn from_xml(e: &Element, font: &mut Font) -> Result<Self> {
		let mut ctx = XmlCtx::default();
		let ctx = &mut ctx;
		Ok(match e.name.as_str() {
			"RearrangementMorph" => MorphSubStruct::Rearrangement(OtTable::xml_read(e, font, ctx)?),
			"ContextualMorph" => MorphSubStruct::Contextual(OtTable::xml_read(e, font, ctx)?),
			"LigatureMorph" => MorphSubStruct::Ligature(OtTable::xml_read(e, font, ctx)?),
			"NoncontextualMorph" => MorphSubStruct::Noncontextual(OtTable::xml_read(e, font, ctx)?),
			"InsertionMorph" => MorphSubStruct::Insertion(OtTable::xml_read(e, font, ctx)?),
			other => return Err(Error::ttx(format!("KeyError: {other:?}"))),
		})
	}
}

/// Reads the subtable after a header of `header` bytes, through a reader of its
/// own data (`MortSubtableConverter.read` / `MorxSubtableConverter.read`).
fn read_sub_struct(
	r: &mut OtReader<'_>,
	font: &mut Font,
	pos: usize,
	length: usize,
	morph_type: u8,
	table: &str,
) -> Result<MorphSubStruct> {
	// The morph type is checked before the length, as in fontTools.
	if !matches!(morph_type, 0 | 1 | 2 | 4 | 5) {
		return Err(assertion(format!(
			"unsupported '{table}' lookup type {morph_type}"
		)));
	}
	let header = r.pos() - pos;
	let data = r.data();
	let start = r.pos().min(data.len());
	let end = (pos + length).clamp(start, data.len());
	let sub = &data[start..end];
	if sub.len() as i64 != length as i64 - header as i64 {
		return Err(assertion("len(data) == m.StructLength - headerLength"));
	}
	let mut reader = OtReader::new(sub, r.table_tag);
	let value = MorphSubStruct::decompile(morph_type, table, &mut reader, font)?;
	r.seek(pos + length);
	Ok(value)
}

fn no_attr(class: &str, name: &str) -> Error {
	Error::ttlib(format!(
		"AttributeError: '{class}' object has no attribute '{name}'"
	))
}

/// Writes the morph type as a count reference that the subtable sets
/// (`MorphType` is a lookup type), then the remaining fields.
fn write_morph_type_and_rest(
	w: &mut OtWriter<'_>,
	font: &mut Font,
	morph_type: Option<u32>,
	sub_feature_flags: Option<i64>,
	sub_struct: Option<&MorphSubStruct>,
	class: &str,
) -> Result<()> {
	let lt = CountRef::new("MorphType", 1, morph_type.map(u64::from));
	w.write_count_reference(lt.clone());
	w.set("LookupType", StateValue::Count(lt));
	let flags = sub_feature_flags.ok_or_else(|| no_attr(class, "SubFeatureFlags"))?;
	w.write_u32(ulong(flags)?);
	let sub_struct = sub_struct.ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'compile'")
	})?;
	sub_struct.compile(w, font)
}

/// A `mort` subtable (`MortSubtable`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MortSubtable {
	/// `StructLength`, as read.
	pub struct_length: Option<u32>,
	/// `CoverageFlags`.
	pub coverage_flags: i64,
	/// `MorphType`, as read; written from the subtable.
	pub morph_type: Option<u32>,
	/// `SubFeatureFlags`.
	pub sub_feature_flags: Option<i64>,
	/// The subtable (`SubStruct`).
	pub sub_struct: Option<MorphSubStruct>,
}

/// `MortSubtableConverter.read`.
pub fn read_mort_subtable(r: &mut OtReader<'_>, font: &mut Font) -> Result<MortSubtable> {
	let pos = r.pos();
	let length = r.read_u16()?;
	let coverage_flags = r.read_u8()? as i64;
	let morph_type = r.read_u8()?;
	let sub_feature_flags = r.read_u32()? as i64;
	let sub_struct = read_sub_struct(r, font, pos, length as usize, morph_type, "mort")?;
	Ok(MortSubtable {
		struct_length: Some(length as u32),
		coverage_flags,
		morph_type: Some(morph_type as u32),
		sub_feature_flags: Some(sub_feature_flags),
		sub_struct: Some(sub_struct),
	})
}

/// `MortSubtableConverter.write`: the length is filled in afterwards.
pub fn write_mort_subtable(
	w: &mut OtWriter<'_>,
	font: &mut Font,
	value: &MortSubtable,
) -> Result<()> {
	let index = w.item_count();
	let before = w.data_length();
	w.write_u16(0xDEAD);
	w.write_u8(
		u8::try_from(value.coverage_flags)
			.map_err(|_| Error::Struct("ubyte format requires 0 <= number <= 255".into()))?,
	);
	write_morph_type_and_rest(
		w,
		font,
		value.morph_type,
		value.sub_feature_flags,
		value.sub_struct.as_ref(),
		"MortSubtable",
	)?;
	let length = w.data_length() - before;
	if length > 0xFFFF {
		return Err(mort_error("mort subtable exceeds 65535 bytes"));
	}
	w.replace_item(index, (length as u16).to_be_bytes().to_vec());
	Ok(())
}

fn struct_length_comment(xw: &mut XmlWriter, length: Option<u32>, class: &str) -> Result<()> {
	let length = length.ok_or_else(|| {
		Error::ttlib(format!(
			"TypeError: %d format: a real number is required, not NoneType ({class})"
		))
	})?;
	xw.comment(&format!("StructLength={length}"));
	xw.newline();
	Ok(())
}

fn morph_type_comment(xw: &mut XmlWriter, value: Option<u32>) -> Result<()> {
	let value = value.ok_or_else(|| {
		Error::ttlib("TypeError: %d format: a real number is required, not NoneType")
	})?;
	xw.comment(&format!("MorphType={value}"));
	xw.newline();
	Ok(())
}

fn sub_feature_flags_xml(xw: &mut XmlWriter, value: Option<i64>, class: &str) -> Result<()> {
	let value = value.ok_or_else(|| no_attr(class, "SubFeatureFlags"))?;
	xw.simpletag("SubFeatureFlags", &[("value", &format!("0x{value:08x}"))]);
	xw.newline();
	Ok(())
}

/// `MortSubtableConverter.xmlWrite`.
pub fn xml_mort_subtable(
	xw: &mut XmlWriter,
	font: &mut Font,
	name: &str,
	attrs: &[(&'static str, String)],
	value: &MortSubtable,
) -> Result<()> {
	super::table::begintag(xw, name, attrs);
	xw.newline();
	struct_length_comment(xw, value.struct_length, "MortSubtable")?;
	xw.simpletag("CoverageFlags", &[("value", &value.coverage_flags)]);
	xw.newline();
	morph_type_comment(xw, value.morph_type)?;
	sub_feature_flags_xml(xw, value.sub_feature_flags, "MortSubtable")?;
	value
		.sub_struct
		.as_ref()
		.ok_or_else(|| no_attr("MortSubtable", "SubStruct"))?
		.to_xml(xw, font)?;
	xw.endtag(name);
	xw.newline();
	Ok(())
}

/// `MortSubtableConverter.xmlRead`.
pub fn read_mort_subtable_xml(element: &Element, font: &mut Font) -> Result<MortSubtable> {
	let mut m = MortSubtable::default();
	for e in element.elements() {
		match e.name.as_str() {
			"CoverageFlags" => m.coverage_flags = eval(attr(e, "value")?)?,
			"SubFeatureFlags" => m.sub_feature_flags = Some(eval(attr(e, "value")?)?),
			name if name.ends_with("Morph") => {
				m.sub_struct = Some(MorphSubStruct::from_xml(e, font)?)
			}
			name => return Err(assertion(name.to_owned())),
		}
	}
	Ok(m)
}

/// The processing orders, by (reverse, logical) bits of the coverage flags.
const PROCESSING_ORDERS: [((bool, bool), &str); 4] = [
	((false, false), "LayoutOrder"),
	((true, false), "ReversedLayoutOrder"),
	((false, true), "LogicalOrder"),
	((true, true), "ReversedLogicalOrder"),
];

fn text_direction(flags: i64) -> &'static str {
	if flags & 0x20 != 0 {
		"Any"
	} else if flags & 0x80 != 0 {
		"Vertical"
	} else {
		"Horizontal"
	}
}

fn processing_order(flags: i64) -> &'static str {
	let key = (flags & 0x40 != 0, flags & 0x10 != 0);
	PROCESSING_ORDERS
		.iter()
		.find(|(k, _)| *k == key)
		.map_or("LayoutOrder", |(_, n)| n)
}

/// A `morx` subtable (`MorxSubtable`); the coverage flags are split into the
/// text direction, processing order and the high bits of `Reserved`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MorxSubtable {
	/// `StructLength`, as read.
	pub struct_length: Option<u32>,
	/// `TextDirection`: `Horizontal`, `Vertical` or `Any`.
	pub text_direction: Option<String>,
	/// `ProcessingOrder`.
	pub processing_order: Option<String>,
	/// `Reserved`, with the low four coverage flag bits in bits 16 to 19.
	pub reserved: i64,
	/// `MorphType`, as read; written from the subtable.
	pub morph_type: Option<u32>,
	/// `SubFeatureFlags`.
	pub sub_feature_flags: Option<i64>,
	/// The subtable (`SubStruct`).
	pub sub_struct: Option<MorphSubStruct>,
}

/// `MorxSubtableConverter.read`.
pub fn read_morx_subtable(r: &mut OtReader<'_>, font: &mut Font) -> Result<MorxSubtable> {
	let pos = r.pos();
	let length = r.read_u32()?;
	let flags = r.read_u8()? as i64;
	let reserved = r.read_u16()? as i64 | ((flags & 0xF) << 16);
	let morph_type = r.read_u8()?;
	let sub_feature_flags = r.read_u32()? as i64;
	let sub_struct = read_sub_struct(r, font, pos, length as usize, morph_type, "morx")?;
	Ok(MorxSubtable {
		struct_length: Some(length),
		text_direction: Some(text_direction(flags).to_owned()),
		processing_order: Some(processing_order(flags).to_owned()),
		reserved,
		morph_type: Some(morph_type as u32),
		sub_feature_flags: Some(sub_feature_flags),
		sub_struct: Some(sub_struct),
	})
}

/// `MorxSubtableConverter.write`: the coverage flags are rebuilt and the length
/// is filled in afterwards.
pub fn write_morx_subtable(
	w: &mut OtWriter<'_>,
	font: &mut Font,
	value: &MorxSubtable,
) -> Result<()> {
	let mut flags = (value.reserved & 0x000F_0000) >> 16;
	let order = value
		.processing_order
		.as_deref()
		.ok_or_else(|| no_attr("MorxSubtable", "ProcessingOrder"))?;
	let ((reverse, logical), _) = PROCESSING_ORDERS
		.iter()
		.find(|(_, n)| *n == order)
		.ok_or_else(|| Error::ttlib(format!("KeyError: {order:?}")))?;
	let direction = value
		.text_direction
		.as_deref()
		.ok_or_else(|| no_attr("MorxSubtable", "TextDirection"))?;
	if direction == "Vertical" {
		flags |= 0x80;
	}
	if *reverse {
		flags |= 0x40;
	}
	if direction == "Any" {
		flags |= 0x20;
	}
	if *logical {
		flags |= 0x10;
	}
	let index = w.item_count();
	let before = w.data_length();
	w.write_u32(0xDEAD_BEEF);
	w.write_u8(
		u8::try_from(flags)
			.map_err(|_| Error::Struct("ubyte format requires 0 <= number <= 255".into()))?,
	);
	w.write_u16(ushort(value.reserved & 0xFFFF)?);
	write_morph_type_and_rest(
		w,
		font,
		value.morph_type,
		value.sub_feature_flags,
		value.sub_struct.as_ref(),
		"MorxSubtable",
	)?;
	let length = w.data_length() - before;
	w.replace_item(index, ulong(length as i64)?.to_be_bytes().to_vec());
	Ok(())
}

/// `MorxSubtableConverter.xmlWrite`.
pub fn xml_morx_subtable(
	xw: &mut XmlWriter,
	font: &mut Font,
	name: &str,
	attrs: &[(&'static str, String)],
	value: &MorxSubtable,
) -> Result<()> {
	super::table::begintag(xw, name, attrs);
	xw.newline();
	struct_length_comment(xw, value.struct_length, "MorxSubtable")?;
	let direction = value
		.text_direction
		.as_deref()
		.ok_or_else(|| no_attr("MorxSubtable", "TextDirection"))?;
	xw.simpletag("TextDirection", &[("value", &direction)]);
	xw.newline();
	let order = value
		.processing_order
		.as_deref()
		.ok_or_else(|| no_attr("MorxSubtable", "ProcessingOrder"))?;
	xw.simpletag("ProcessingOrder", &[("value", &order)]);
	xw.newline();
	if value.reserved != 0 {
		xw.simpletag(
			"Reserved",
			&[("value", &format!("0x{:04x}", value.reserved))],
		);
		xw.newline();
	}
	morph_type_comment(xw, value.morph_type)?;
	sub_feature_flags_xml(xw, value.sub_feature_flags, "MorxSubtable")?;
	value
		.sub_struct
		.as_ref()
		.ok_or_else(|| no_attr("MorxSubtable", "SubStruct"))?
		.to_xml(xw, font)?;
	xw.endtag(name);
	xw.newline();
	Ok(())
}

/// `MorxSubtableConverter.xmlRead`.
pub fn read_morx_subtable_xml(element: &Element, font: &mut Font) -> Result<MorxSubtable> {
	let mut m = MorxSubtable::default();
	let mut coverage_flags = 0;
	for e in element.elements() {
		match e.name.as_str() {
			// Only in TTX from old versions of fontTools.
			"CoverageFlags" => {
				coverage_flags = eval(attr(e, "value")?)?;
				m.processing_order = Some(processing_order(coverage_flags).to_owned());
				m.text_direction = Some(text_direction(coverage_flags).to_owned());
			}
			"ProcessingOrder" => {
				let v = attr(e, "value")?;
				if !PROCESSING_ORDERS.iter().any(|(_, n)| *n == v) {
					return Err(assertion(format!("unknown ProcessingOrder: {v}")));
				}
				m.processing_order = Some(v.to_owned());
			}
			"TextDirection" => {
				let v = attr(e, "value")?;
				if !["Horizontal", "Vertical", "Any"].contains(&v) {
					return Err(assertion(format!("unknown TextDirection {v}")));
				}
				m.text_direction = Some(v.to_owned());
			}
			"Reserved" => m.reserved = eval(attr(e, "value")?)?,
			"SubFeatureFlags" => m.sub_feature_flags = Some(eval(attr(e, "value")?)?),
			name if name.ends_with("Morph") => {
				m.sub_struct = Some(MorphSubStruct::from_xml(e, font)?)
			}
			name => return Err(assertion(name.to_owned())),
		}
	}
	m.reserved |= (coverage_flags & 0xF) << 16;
	Ok(m)
}

impl AatStateTable {
	/// Maps the glyphs of the class lookup and of the per-glyph lookups.
	pub fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		self.glyph_classes = super::aat::remap_keys(std::mem::take(&mut self.glyph_classes), f)?;
		for lookup in &mut self.per_glyph_lookups {
			super::aat::remap_aat_lookup(lookup, f)?;
		}
		Ok(())
	}
}

impl MortSubtable {
	/// Not supported: fontTools cannot subset `mort` either, so it never needs to
	/// follow a new glyph order.
	pub fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Err(Error::ttlib(
			"remapping the glyphs of 'mort' subtables is not supported",
		))
	}
}

impl MorxSubtable {
	/// Not supported: fontTools cannot subset `morx` either, so it never needs to
	/// follow a new glyph order.
	pub fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Err(Error::ttlib(
			"remapping the glyphs of 'morx' subtables is not supported",
		))
	}
}
