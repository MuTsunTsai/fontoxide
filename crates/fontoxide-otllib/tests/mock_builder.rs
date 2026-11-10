//! `Tests/otlLib/mock_builder_test.py`.

use std::cell::RefCell;
use std::sync::Once;

use fontoxide_otllib::builder::{ChainContextualRule, LookupBuilder, RuleLookup};
use fontoxide_otllib::{Error, GlyphMap, Result};
use fontoxide_ttlib::Font;

fn ttfont() -> Font {
	let mut glyphs: Vec<String> = "
		.notdef space slash fraction semicolon period comma ampersand
		quotedblleft quotedblright quoteleft quoteright
		zero one two three four five six seven eight nine
		zero.oldstyle one.oldstyle two.oldstyle three.oldstyle
		four.oldstyle five.oldstyle six.oldstyle seven.oldstyle
		eight.oldstyle nine.oldstyle onequarter onehalf threequarters
		onesuperior twosuperior threesuperior ordfeminine ordmasculine
		A B C D E F G H I J K L M N O P Q R S T U V W X Y Z
		a b c d e f g h i j k l m n o p q r s t u v w x y z
		A.sc B.sc C.sc D.sc E.sc F.sc G.sc H.sc I.sc J.sc K.sc L.sc M.sc
		N.sc O.sc P.sc Q.sc R.sc S.sc T.sc U.sc V.sc W.sc X.sc Y.sc Z.sc
		A.alt1 A.alt2 A.alt3 B.alt1 B.alt2 B.alt3 C.alt1 C.alt2 C.alt3
		a.alt1 a.alt2 a.alt3 a.end b.alt c.mid d.alt d.mid
		e.begin e.mid e.end m.begin n.end s.end z.end
		Eng Eng.alt1 Eng.alt2 Eng.alt3
		A.swash B.swash C.swash D.swash E.swash F.swash G.swash H.swash
		I.swash J.swash K.swash L.swash M.swash N.swash O.swash P.swash
		Q.swash R.swash S.swash T.swash U.swash V.swash W.swash X.swash
		Y.swash Z.swash
		f_l c_h c_k c_s c_t f_f f_f_i f_f_l f_i o_f_f_i s_t f_i.begin
		a_n_d T_h T_h.swash germandbls ydieresis yacute breve
		grave acute dieresis macron circumflex cedilla umlaut ogonek caron
		damma hamza sukun kasratan lam_meem_jeem noon.final noon.initial
		by feature lookup sub table uni0327 uni0328 e.fina
	"
	.split_whitespace()
	.map(str::to_owned)
	.collect();
	glyphs.extend((800..=1001).map(|cid| format!("cid{cid:05}")));
	let mut font = Font::new();
	font.set_glyph_order(glyphs);
	font
}

thread_local! {
	static CAPTURED: RefCell<Option<String>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		CAPTURED.with(|c| {
			if let Some(text) = c.borrow_mut().as_mut() {
				text.push_str(&record.args().to_string());
				text.push('\n');
			}
		});
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

/// What `f` logs on this thread at level INFO and above (`CapturingLogHandler`).
fn capture<R>(f: impl FnOnce() -> R) -> (String, R) {
	INIT.call_once(|| {
		log::set_logger(&CAPTURE).expect("no other logger");
		log::set_max_level(log::LevelFilter::Info);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some(String::new()));
	let result = f();
	let text = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
	(text, result)
}

/// The string of `MockBuilderLocation((n, name))`.
fn location(n: u32, name: &str) -> Option<String> {
	Some(format!("{n}:{name}"))
}

#[test]
fn test_unsupported_subtable_break_1() -> Result<()> {
	let mut font = ttfont();
	let gm = GlyphMap::from_font(&mut font)?;
	let (log, result) = capture(|| {
		let mut builder = LookupBuilder::single_pos(gm, location(0, "alpha"));
		builder.add_subtable_break(&location(5, "beta"));
		builder.build(&mut font)
	});
	result?;
	assert!(
		log.contains("5:beta: unsupported \"subtable\" statement for lookup type"),
		"{log}"
	);
	Ok(())
}

#[test]
fn test_chain_pos_references_gsub_lookup() -> Result<()> {
	let mut font = ttfont();
	let gm = GlyphMap::from_font(&mut font)?;
	let mut builder = LookupBuilder::chain_context_pos(gm.clone(), location(0, "alpha"));
	let builder2 = LookupBuilder::single_subst(gm, location(0, "alpha")).into_ref();
	if let LookupBuilder::ChainContextPos(b) = &mut builder {
		b.rules.push(ChainContextualRule::new(
			vec![],
			vec![],
			vec![],
			vec![Some(RuleLookup::Many(vec![builder2]))],
		));
	}
	let err = builder.build(&mut font).expect_err("missing index");
	assert!(matches!(&err, Error::OpenTypeLib { .. }));
	assert!(
		err.to_string().contains(
			"0:alpha: Missing index of the specified lookup, might be a substitution lookup"
		),
		"{err}"
	);
	Ok(())
}
