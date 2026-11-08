//! Assembler and disassembler for TrueType bytecode
//! (`fontTools.ttLib.tables.ttProgram`), used by `fpgm`, `prep` and `glyf`.

use fontoxide_misc::text_tools::{binary2num, num2binary, read_hex};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::{Error, Result};

/// An instruction: opcode, mnemonic, argument bits and descriptive name.
type Instruction = (u8, &'static str, u32, &'static str);

/// Instructions that read bytes or words from the instruction stream.
const STREAM_INSTRUCTIONS: [Instruction; 4] = [
	(0x40, "NPUSHB", 0, "PushNBytes"),
	(0x41, "NPUSHW", 0, "PushNWords"),
	(0xb0, "PUSHB", 3, "PushBytes"),
	(0xb8, "PUSHW", 3, "PushWords"),
];

/// The other instructions.
const INSTRUCTIONS: &[Instruction] = &[
	(0x7f, "AA", 0, "AdjustAngle"),
	(0x64, "ABS", 0, "Absolute"),
	(0x60, "ADD", 0, "Add"),
	(0x27, "ALIGNPTS", 0, "AlignPts"),
	(0x3c, "ALIGNRP", 0, "AlignRelativePt"),
	(0x5a, "AND", 0, "LogicalAnd"),
	(0x2b, "CALL", 0, "CallFunction"),
	(0x67, "CEILING", 0, "Ceiling"),
	(0x25, "CINDEX", 0, "CopyXToTopStack"),
	(0x22, "CLEAR", 0, "ClearStack"),
	(0x4f, "DEBUG", 0, "DebugCall"),
	(0x73, "DELTAC1", 0, "DeltaExceptionC1"),
	(0x74, "DELTAC2", 0, "DeltaExceptionC2"),
	(0x75, "DELTAC3", 0, "DeltaExceptionC3"),
	(0x5d, "DELTAP1", 0, "DeltaExceptionP1"),
	(0x71, "DELTAP2", 0, "DeltaExceptionP2"),
	(0x72, "DELTAP3", 0, "DeltaExceptionP3"),
	(0x24, "DEPTH", 0, "GetDepthStack"),
	(0x62, "DIV", 0, "Divide"),
	(0x20, "DUP", 0, "DuplicateTopStack"),
	(0x59, "EIF", 0, "EndIf"),
	(0x1b, "ELSE", 0, "Else"),
	(0x2d, "ENDF", 0, "EndFunctionDefinition"),
	(0x54, "EQ", 0, "Equal"),
	(0x57, "EVEN", 0, "Even"),
	(0x2c, "FDEF", 0, "FunctionDefinition"),
	(0x4e, "FLIPOFF", 0, "SetAutoFlipOff"),
	(0x4d, "FLIPON", 0, "SetAutoFlipOn"),
	(0x80, "FLIPPT", 0, "FlipPoint"),
	(0x82, "FLIPRGOFF", 0, "FlipRangeOff"),
	(0x81, "FLIPRGON", 0, "FlipRangeOn"),
	(0x66, "FLOOR", 0, "Floor"),
	(0x46, "GC", 1, "GetCoordOnPVector"),
	(0x88, "GETINFO", 0, "GetInfo"),
	(0x91, "GETVARIATION", 0, "GetVariation"),
	(0x0d, "GFV", 0, "GetFVector"),
	(0x0c, "GPV", 0, "GetPVector"),
	(0x52, "GT", 0, "GreaterThan"),
	(0x53, "GTEQ", 0, "GreaterThanOrEqual"),
	(0x89, "IDEF", 0, "InstructionDefinition"),
	(0x58, "IF", 0, "If"),
	(0x8e, "INSTCTRL", 0, "SetInstrExecControl"),
	(0x39, "IP", 0, "InterpolatePts"),
	(0x0f, "ISECT", 0, "MovePtToIntersect"),
	(0x30, "IUP", 1, "InterpolateUntPts"),
	(0x1c, "JMPR", 0, "Jump"),
	(0x79, "JROF", 0, "JumpRelativeOnFalse"),
	(0x78, "JROT", 0, "JumpRelativeOnTrue"),
	(0x2a, "LOOPCALL", 0, "LoopAndCallFunction"),
	(0x50, "LT", 0, "LessThan"),
	(0x51, "LTEQ", 0, "LessThenOrEqual"),
	(0x8b, "MAX", 0, "Maximum"),
	(0x49, "MD", 1, "MeasureDistance"),
	(0x2e, "MDAP", 1, "MoveDirectAbsPt"),
	(0xc0, "MDRP", 5, "MoveDirectRelPt"),
	(0x3e, "MIAP", 1, "MoveIndirectAbsPt"),
	(0x8c, "MIN", 0, "Minimum"),
	(0x26, "MINDEX", 0, "MoveXToTopStack"),
	(0xe0, "MIRP", 5, "MoveIndirectRelPt"),
	(0x4b, "MPPEM", 0, "MeasurePixelPerEm"),
	(0x4c, "MPS", 0, "MeasurePointSize"),
	(0x3a, "MSIRP", 1, "MoveStackIndirRelPt"),
	(0x63, "MUL", 0, "Multiply"),
	(0x65, "NEG", 0, "Negate"),
	(0x55, "NEQ", 0, "NotEqual"),
	(0x5c, "NOT", 0, "LogicalNot"),
	(0x6c, "NROUND", 2, "NoRound"),
	(0x56, "ODD", 0, "Odd"),
	(0x5b, "OR", 0, "LogicalOr"),
	(0x21, "POP", 0, "PopTopStack"),
	(0x45, "RCVT", 0, "ReadCVT"),
	(0x7d, "RDTG", 0, "RoundDownToGrid"),
	(0x7a, "ROFF", 0, "RoundOff"),
	(0x8a, "ROLL", 0, "RollTopThreeStack"),
	(0x68, "ROUND", 2, "Round"),
	(0x43, "RS", 0, "ReadStore"),
	(0x3d, "RTDG", 0, "RoundToDoubleGrid"),
	(0x18, "RTG", 0, "RoundToGrid"),
	(0x19, "RTHG", 0, "RoundToHalfGrid"),
	(0x7c, "RUTG", 0, "RoundUpToGrid"),
	(0x77, "S45ROUND", 0, "SuperRound45Degrees"),
	(0x7e, "SANGW", 0, "SetAngleWeight"),
	(0x85, "SCANCTRL", 0, "ScanConversionControl"),
	(0x8d, "SCANTYPE", 0, "ScanType"),
	(0x48, "SCFS", 0, "SetCoordFromStackFP"),
	(0x1d, "SCVTCI", 0, "SetCVTCutIn"),
	(0x5e, "SDB", 0, "SetDeltaBaseInGState"),
	(0x86, "SDPVTL", 1, "SetDualPVectorToLine"),
	(0x5f, "SDS", 0, "SetDeltaShiftInGState"),
	(0x0b, "SFVFS", 0, "SetFVectorFromStack"),
	(0x04, "SFVTCA", 1, "SetFVectorToAxis"),
	(0x08, "SFVTL", 1, "SetFVectorToLine"),
	(0x0e, "SFVTPV", 0, "SetFVectorToPVector"),
	(0x34, "SHC", 1, "ShiftContourByLastPt"),
	(0x32, "SHP", 1, "ShiftPointByLastPoint"),
	(0x38, "SHPIX", 0, "ShiftZoneByPixel"),
	(0x36, "SHZ", 1, "ShiftZoneByLastPoint"),
	(0x17, "SLOOP", 0, "SetLoopVariable"),
	(0x1a, "SMD", 0, "SetMinimumDistance"),
	(0x0a, "SPVFS", 0, "SetPVectorFromStack"),
	(0x02, "SPVTCA", 1, "SetPVectorToAxis"),
	(0x06, "SPVTL", 1, "SetPVectorToLine"),
	(0x76, "SROUND", 0, "SuperRound"),
	(0x10, "SRP0", 0, "SetRefPoint0"),
	(0x11, "SRP1", 0, "SetRefPoint1"),
	(0x12, "SRP2", 0, "SetRefPoint2"),
	(0x1f, "SSW", 0, "SetSingleWidth"),
	(0x1e, "SSWCI", 0, "SetSingleWidthCutIn"),
	(0x61, "SUB", 0, "Subtract"),
	(0x00, "SVTCA", 1, "SetFPVectorToAxis"),
	(0x23, "SWAP", 0, "SwapTopStack"),
	(0x13, "SZP0", 0, "SetZonePointer0"),
	(0x14, "SZP1", 0, "SetZonePointer1"),
	(0x15, "SZP2", 0, "SetZonePointer2"),
	(0x16, "SZPS", 0, "SetZonePointerS"),
	(0x29, "UTP", 0, "UnTouchPt"),
	(0x70, "WCVTF", 0, "WriteCVTInFUnits"),
	(0x44, "WCVTP", 0, "WriteCVTInPixels"),
	(0x42, "WS", 0, "WriteStore"),
];

/// The instruction an opcode belongs to, and the opcode of its argument 0
/// (`opcodeDict` / `streamOpcodeDict`).
fn lookup_opcode(table: &[Instruction], op: u8) -> Option<(Instruction, u8)> {
	table.iter().find_map(|&ins| {
		let (base, _, bits, _) = ins;
		let count = 1u16 << bits;
		(op >= base && (op as u16) < base as u16 + count).then_some((ins, base))
	})
}

fn lookup_mnemonic(table: &[Instruction], mnemonic: &str) -> Option<Instruction> {
	table.iter().copied().find(|ins| ins.1 == mnemonic)
}

fn error(msg: impl Into<String>) -> Error {
	Error::ttlib(format!("TT instructions error: {:?}", msg.into()))
}

/// `str.isspace`, which `\s` follows.
fn is_space(c: char) -> bool {
	c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// A token of assembly (`_tokenRE`).
enum Token<'a> {
	Instruction { mnemonic: &'a str, arg: &'a str },
	Number(&'a str),
	Comment,
}

/// Matches a token at `pos`, returning it and the position after it.
fn token(s: &str, pos: usize) -> Option<(Token<'_>, usize)> {
	let rest = &s[pos..];
	let bytes = rest.as_bytes();
	// ([A-Z][A-Z0-9]*)\s*\[(.*?)\]
	if bytes.first().is_some_and(u8::is_ascii_uppercase) {
		let end = bytes
			.iter()
			.position(|b| !(b.is_ascii_uppercase() || b.is_ascii_digit()))
			.unwrap_or(bytes.len());
		let mnemonic = &rest[..end];
		let after = rest[end..].trim_start_matches(is_space);
		if let Some(arg_and_more) = after.strip_prefix('[')
			&& let Some(close) = arg_and_more.find(']')
		{
			let arg = &arg_and_more[..close];
			if !arg.contains('\n') {
				let consumed = rest.len() - arg_and_more.len() + close + 1;
				return Some((Token::Instruction { mnemonic, arg }, pos + consumed));
			}
		}
	}
	// -?[0-9]+
	let digits_start = usize::from(bytes.first() == Some(&b'-'));
	let digits = bytes[digits_start..]
		.iter()
		.take_while(|b| b.is_ascii_digit())
		.count();
	if digits > 0 {
		let end = digits_start + digits;
		return Some((Token::Number(&rest[..end]), pos + end));
	}
	// /\*.*?\*/
	if let Some(body) = rest.strip_prefix("/*")
		&& let Some(close) = body.find("*/")
		&& !body[..close].contains('\n')
	{
		return Some((Token::Comment, pos + 2 + close + 2));
	}
	None
}

fn skip_white(s: &str, pos: usize) -> usize {
	pos + s[pos..].len() - s[pos..].trim_start_matches(is_space).len()
}

/// `str.splitlines`, then joined with spaces.
fn join_lines(text: &str) -> String {
	let mut lines: Vec<&str> = Vec::new();
	let mut start = 0;
	let mut chars = text.char_indices().peekable();
	while let Some((i, c)) = chars.next() {
		let is_break = matches!(
			c,
			'\n' | '\r'
				| '\x0b' | '\x0c'
				| '\x1c' | '\x1d'
				| '\x1e' | '\u{85}'
				| '\u{2028}' | '\u{2029}'
		);
		if is_break {
			lines.push(&text[start..i]);
			let mut next = i + c.len_utf8();
			if c == '\r' && chars.peek().map(|&(_, n)| n) == Some('\n') {
				chars.next();
				next += 1;
			}
			start = next;
		}
	}
	if start < text.len() {
		lines.push(&text[start..]);
	}
	lines.join(" ")
}

fn parse_int(s: &str) -> Result<i64> {
	s.parse::<i64>()
		.map_err(|_| Error::value(format!("invalid literal for int() with base 10: {s:?}")))
}

/// A TrueType program (`Program`), held as bytecode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Program {
	/// The bytecode.
	pub bytecode: Vec<u8>,
}

impl Program {
	/// A program from bytecode (`fromBytecode`).
	pub fn from_bytecode(bytecode: &[u8]) -> Self {
		Program {
			bytecode: bytecode.to_vec(),
		}
	}

	/// Assembles a program (`fromAssembly` + `_assemble`).
	pub fn from_assembly<S: AsRef<str>>(lines: &[S]) -> Result<Self> {
		let assembly = lines
			.iter()
			.map(AsRef::as_ref)
			.collect::<Vec<_>>()
			.join(" ");
		Ok(Program {
			bytecode: assemble(&assembly)?,
		})
	}

	/// Whether the program has any instructions (`__bool__`).
	pub fn is_empty(&self) -> bool {
		self.bytecode.is_empty()
	}

	/// The assembly (`getAssembly`); with `preserve`, the push instructions
	/// are kept as they are, otherwise consecutive ones are merged into `PUSH`.
	pub fn assembly(&self, preserve: bool) -> Result<Vec<String>> {
		disassemble(&self.bytecode, preserve)
	}

	/// Writes the program as TTX (`toXML`): its assembly, or its bytecode with
	/// the error if it cannot be disassembled.
	pub fn to_xml(&self, writer: &mut XmlWriter) {
		let assembly = match self.assembly(true) {
			Ok(a) => a,
			Err(e) => {
				// LENIENT(L10): fontTools writes the Python traceback here.
				let msg = format!(
					"An exception occurred during the decompilation of glyph program:\n\n{e}"
				);
				log::error!("{msg}");
				writer.begintag("bytecode", &[]);
				writer.newline();
				writer.comment(msg.trim());
				writer.newline();
				writer.dumphex(&self.bytecode);
				writer.endtag("bytecode");
				writer.newline();
				return;
			}
		};
		if assembly.is_empty() {
			return;
		}
		let white = writer.indent_white().to_owned();
		writer.begintag("assembly", &[]);
		writer.newline();
		let mut i = 0;
		let mut indent: i64 = 0;
		while i < assembly.len() {
			let instr = &assembly[i];
			if unindents(instr) {
				indent -= 1;
			}
			writer.write(&white.repeat(indent.max(0) as usize));
			writer.write(instr);
			writer.newline();
			i += 1;
			if let Some(n) = push_count(instr) {
				let mut line: Vec<&str> = Vec::new();
				let mut j = 0;
				for k in 0..n {
					j = k;
					if k > 0 && k % 25 == 0 {
						writer.write(&white.repeat(indent.max(0) as usize));
						writer.write(&line.join(" "));
						writer.newline();
						line.clear();
					}
					line.push(assembly.get(i + k).map_or("", String::as_str));
				}
				writer.write(&white.repeat(indent.max(0) as usize));
				writer.write(&line.join(" "));
				writer.newline();
				i += j + 1;
			}
			if indents(instr) {
				indent += 1;
			}
		}
		writer.endtag("assembly");
		writer.newline();
	}

	/// Reads an `assembly` or `bytecode` element (`fromXML`).
	pub fn from_xml(element: &Element) -> Result<Self> {
		match element.name.as_str() {
			"assembly" => {
				let text = element.text();
				Ok(Program {
					bytecode: assemble(&join_lines(&text))?,
				})
			}
			"bytecode" => {
				let data = read_hex(element.text_chunks())
					.ok_or_else(|| Error::value("non-hexadecimal number found"))?;
				Ok(Program { bytecode: data })
			}
			other => Err(Error::Assertion(format!("name == 'bytecode' ({other:?})"))),
		}
	}
}

/// `_indentRE` (`^FDEF|IDEF|IF|ELSE\[ \]\t.+`, all matched at the start).
fn indents(instr: &str) -> bool {
	instr.starts_with("FDEF")
		|| instr.starts_with("IDEF")
		|| instr.starts_with("IF")
		|| instr
			.strip_prefix("ELSE[ ]\t")
			.is_some_and(|rest| rest.chars().any(|c| c != '\n'))
}

/// `_unindentRE` (`^ELSE|ENDF|EIF\[ \]\t.+`, all matched at the start).
fn unindents(instr: &str) -> bool {
	instr.starts_with("ELSE")
		|| instr.starts_with("ENDF")
		|| instr
			.strip_prefix("EIF[ ]\t")
			.is_some_and(|rest| rest.chars().any(|c| c != '\n'))
}

/// `_pushCountPat`: the number of values of a push line, from its comment.
fn push_count(instr: &str) -> Option<usize> {
	// [A-Z][A-Z0-9]*\s*\[.*?\]\s*/\* ([0-9]+).*?\*/
	let (tok, end) = token(instr, 0)?;
	if !matches!(tok, Token::Instruction { .. }) {
		return None;
	}
	let rest = instr[end..].trim_start_matches(is_space);
	let rest = rest.strip_prefix("/* ")?;
	let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
	if digits.is_empty() || !rest[digits.len()..].contains("*/") {
		return None;
	}
	digits.parse().ok()
}

/// `_assemble`.
fn assemble(assembly: &str) -> Result<Vec<u8>> {
	let mut out: Vec<i64> = Vec::new();
	let len = assembly.len();
	let mut pos = skip_white(assembly, 0);
	let around = |from: usize, to: usize| {
		let from = assembly.floor_char_boundary(from.min(len));
		let to = assembly.floor_char_boundary(to.min(len));
		assembly[from..to.max(from)].to_owned()
	};
	while pos < len {
		let (tok, next) = token(assembly, pos).ok_or_else(|| {
			error(format!(
				"Syntax error in TT program ({})",
				around(pos.saturating_sub(5), pos + 15)
			))
		})?;
		pos = next;
		let (mnemonic, arg) = match tok {
			Token::Comment => {
				pos = skip_white(assembly, pos);
				continue;
			}
			Token::Number(_) => {
				return Err(Error::ttlib(
					"AttributeError: 'NoneType' object has no attribute 'strip'",
				));
			}
			Token::Instruction { mnemonic, arg } => (mnemonic, arg.trim_matches(is_space)),
		};
		if let Some(code) = mnemonic.strip_prefix("INSTR") {
			// An unknown instruction.
			out.push(parse_int(code)?);
		} else if !matches!(mnemonic, "PUSH" | "NPUSHB" | "NPUSHW" | "PUSHB" | "PUSHW") {
			let (op, _, bits, _) = lookup_mnemonic(INSTRUCTIONS, mnemonic)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {mnemonic:?}")))?;
			if arg.chars().count() != bits as usize {
				return Err(error(format!(
					"Incorrect number of argument bits ({mnemonic}[{arg}])"
				)));
			}
			out.push(op as i64 + if arg.is_empty() { 0 } else { binary2num(arg) });
		} else {
			let mut args: Vec<i64> = Vec::new();
			pos = skip_white(assembly, pos);
			while pos < len {
				let (tok, next) = token(assembly, pos).ok_or_else(|| {
					error(format!(
						"Syntax error in TT program ({})",
						around(pos, pos + 15)
					))
				})?;
				match tok {
					Token::Instruction { .. } => break,
					Token::Comment => {
						pos = skip_white(assembly, next);
					}
					Token::Number(n) => {
						pos = skip_white(assembly, next);
						args.push(parse_int(n)?);
					}
				}
			}
			push_args(&mut out, mnemonic, args)?;
		}
		pos = skip_white(assembly, pos);
	}
	out.into_iter()
		.map(|b| {
			u8::try_from(b)
				.map_err(|_| Error::Assertion("max(bytecode) < 256 and min(bytecode) >= 0".into()))
		})
		.collect()
}

fn push_word(out: &mut Vec<i64>, value: i64, what: &str) -> Result<()> {
	if !(-32768..32768).contains(&value) {
		return Err(Error::Assertion(format!(
			"{what} value out of range {value}"
		)));
	}
	out.push((value >> 8) & 0xFF);
	out.push(value & 0xFF);
	Ok(())
}

fn stream_op(mnemonic: &str) -> i64 {
	lookup_mnemonic(&STREAM_INSTRUCTIONS, mnemonic).map_or(0, |i| i.0 as i64)
}

/// Encodes the values of a push instruction; `PUSH` picks the most compact
/// instructions.
fn push_args(out: &mut Vec<i64>, mnemonic: &str, mut args: Vec<i64>) -> Result<()> {
	if mnemonic == "PUSH" {
		let is_byte = |v: i64| (0..=255).contains(&v);
		let mut n_words = 0;
		while !args.is_empty() {
			let n = args.len();
			while n_words < n && n_words < 255 && !is_byte(args[n_words]) {
				n_words += 1;
			}
			let mut n_bytes = 0;
			while n_words + n_bytes < n && n_bytes < 255 && is_byte(args[n_words + n_bytes]) {
				n_bytes += 1;
			}
			if n_bytes < 2 && n_words + n_bytes < 255 && n_words + n_bytes != n {
				// Write the bytes as words.
				n_words += n_bytes;
				continue;
			}
			if n_words > 0 {
				if n_words <= 8 {
					out.push(stream_op("PUSHW") + n_words as i64 - 1);
				} else {
					out.push(stream_op("NPUSHW"));
					out.push(n_words as i64);
				}
				for &v in &args[..n_words] {
					push_word(out, v, "PUSH")?;
				}
			}
			if n_bytes > 0 {
				if n_bytes <= 8 {
					out.push(stream_op("PUSHB") + n_bytes as i64 - 1);
				} else {
					out.push(stream_op("NPUSHB"));
					out.push(n_bytes as i64);
				}
				out.extend(&args[n_words..n_words + n_bytes]);
			}
			args.drain(..n_words + n_bytes);
			n_words = 0;
		}
		return Ok(());
	}
	let words = mnemonic.ends_with('W');
	let op = stream_op(mnemonic);
	let n = args.len() as i64;
	if !mnemonic.starts_with('N') {
		if n > 8 {
			return Err(Error::Assertion(n.to_string()));
		}
		out.push(op + n - 1);
	} else {
		if n >= 256 {
			return Err(Error::Assertion(n.to_string()));
		}
		out.push(op);
		out.push(n);
	}
	for v in args {
		if words {
			push_word(out, v, "PUSHW")?;
		} else {
			if !(0..256).contains(&v) {
				return Err(Error::Assertion(format!("PUSHB value out of range {v}")));
			}
			out.push(v);
		}
	}
	Ok(())
}

/// `_disassemble`.
fn disassemble(bytecode: &[u8], preserve: bool) -> Result<Vec<String>> {
	let index = || Error::ttlib("IndexError: array index out of range");
	let mut assembly = Vec::new();
	let mut i = 0;
	while i < bytecode.len() {
		let op = bytecode[i];
		if let Some(((_, mnemonic, bits, name), base)) = lookup_opcode(INSTRUCTIONS, op) {
			if bits > 0 {
				assembly.push(format!(
					"{mnemonic}[{}]\t/* {name} */",
					num2binary((op - base) as i64, bits)
				));
			} else {
				assembly.push(format!("{mnemonic}[ ]\t/* {name} */"));
			}
			i += 1;
			continue;
		}
		if lookup_opcode(&STREAM_INSTRUCTIONS, op).is_none() {
			assembly.push(format!("INSTR{op}[ ]"));
			i += 1;
			continue;
		}
		let mut values = Vec::new();
		let mut mnemonic = "";
		// Merges consecutive pushes unless preserving them.
		while let Some(((_, m, bits, _), base)) =
			lookup_opcode(&STREAM_INSTRUCTIONS, *bytecode.get(i).ok_or_else(index)?)
		{
			let op = bytecode[i];
			mnemonic = m;
			let words = m.ends_with('W');
			let n = if bits > 0 {
				(op - base) as usize + 1
			} else {
				i += 1;
				*bytecode.get(i).ok_or_else(index)? as usize
			};
			i += 1;
			if n == 0 {
				return Err(Error::Assertion("nValues > 0".into()));
			}
			for _ in 0..n {
				if words {
					let hi = *bytecode.get(i).ok_or_else(index)? as i32;
					let lo = *bytecode.get(i + 1).ok_or_else(index)? as i32;
					let mut v = (hi << 8) | lo;
					if v >= 0x8000 {
						v -= 0x10000;
					}
					values.push(v.to_string());
					i += 2;
				} else {
					values.push(bytecode.get(i).ok_or_else(index)?.to_string());
					i += 1;
				}
			}
			if preserve {
				break;
			}
		}
		let mnemonic = if preserve { mnemonic } else { "PUSH" };
		if values.len() == 1 {
			assembly.push(format!("{mnemonic}[ ]\t/* 1 value pushed */"));
		} else {
			assembly.push(format!(
				"{mnemonic}[ ]\t/* {} values pushed */",
				values.len()
			));
		}
		assembly.extend(values);
	}
	Ok(assembly)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trips() -> Result<()> {
		let bytecode = [
			0xb1, 0x01, 0x02, 0x40, 0x0a, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0xb8, 0xff, 0xfe, 0x00,
			0x58, 0x1b, 0x59, 0xc5, 0x92,
		];
		let p = Program::from_bytecode(&bytecode);
		let asm = p.assembly(true)?;
		assert_eq!(asm[0], "PUSHB[ ]\t/* 2 values pushed */");
		assert_eq!(asm[3], "NPUSHB[ ]\t/* 10 values pushed */");
		assert!(asm.contains(&"PUSHW[ ]\t/* 1 value pushed */".to_owned()));
		assert!(asm.contains(&"-2".to_owned()));
		assert!(asm.contains(&"MDRP[00101]\t/* MoveDirectRelPt */".to_owned()));
		assert!(asm.contains(&"INSTR146[ ]".to_owned()));
		assert_eq!(Program::from_assembly(&asm)?.bytecode, bytecode);
		Ok(())
	}

	#[test]
	fn push_picks_compact_encoding() -> Result<()> {
		let p = Program::from_assembly(&["PUSH[ ] 1 2 300 4 5"])?;
		assert_eq!(p.bytecode, [0xb1, 1, 2, 0xb8, 0x01, 0x2c, 0xb1, 4, 5]);
		Ok(())
	}
}
