//! `Tests/feaLib/ast_test.py` and the data part of
//! `Tests/feaLib/variableScalar_test.py`.

use fontoxide_fealib::ast::*;
use fontoxide_fealib::lexer::FeatureSource;
use fontoxide_fealib::parser::Parser;
use fontoxide_fealib::variable_scalar::VariableScalar;
use fontoxide_fealib::{FeatureLibLocation, Result};

#[test]
fn test_glyphname_escape() {
	let mut statement = GlyphClass::default();
	for name in ["BASE", "NULL", "foo", "a"] {
		statement.append(GlyphName::new(name).glyph);
	}
	assert_eq!(statement.as_fea(), r"[\BASE \NULL foo a]");
}

#[test]
fn test_valuerecord_none() {
	let statement = ValueRecord {
		x_placement: Some(ScalarValue::Int(10)),
		x_advance: Some(ScalarValue::Int(20)),
		..Default::default()
	};
	assert_eq!(statement.as_fea(), "<10 0 20 0>");
}

#[test]
fn test_valuerecord_empty() {
	let statement = ValueRecord::default();
	assert_eq!(statement.as_fea(), "<NULL>");
}

#[test]
fn test_non_object_location() {
	let location = FeatureLibLocation::from(("file.fea", 1, 2));
	assert_eq!(&*location.file, "file.fea");
	assert_eq!(location.line, 1);
	assert_eq!(location.column, 2);
}

fn single_pos(force_chain: bool) -> SinglePosStatement {
	SinglePosStatement {
		pos: vec![(
			GlyphContainer::GlyphName(GlyphName::new("a")),
			Some(ValueRecord::default()),
		)],
		prefix: vec![],
		suffix: vec![],
		force_chain,
		location: None,
	}
}

#[test]
fn test_single_pos_statement_empty_valuerecord() {
	assert_eq!(single_pos(false).as_fea(), "pos a <NULL>;");
}

#[test]
fn test_single_pos_statement_empty_valuerecord_chain() {
	assert_eq!(single_pos(true).as_fea(), "pos a' <NULL>;");
}

fn feature_name(tag: &str, pid: i64, eid: i64, lid: i64, string: &str) -> FeatureNameStatement {
	FeatureNameStatement {
		name_id: tag.to_owned(),
		platform_id: pid,
		plat_enc_id: eid,
		lang_id: lid,
		string: string.to_owned(),
		location: None,
	}
}

fn stat_name(pid: i64, eid: i64, lid: i64, string: &str) -> STATNameStatement {
	STATNameStatement {
		name_id: "stat".to_owned(),
		platform_id: pid,
		plat_enc_id: eid,
		lang_id: lid,
		string: string.to_owned(),
		location: None,
	}
}

#[test]
fn test_name_statements_escape_quote_and_backslash() {
	let string = "a\"b\\c";
	let win = r#"name "a\0022b\005cc";"#;
	let mac = r#"name 1 "a\22b\5cc";"#;
	let cv = CVParametersNameStatement {
		name_id: "cv01".to_owned(),
		platform_id: 3,
		plat_enc_id: 1,
		lang_id: 0x409,
		string: string.to_owned(),
		block_name: "FeatUILabelNameID".to_owned(),
		location: None,
	};
	let cases = [
		(
			feature_name("ss01", 3, 1, 0x409, string).as_fea(),
			win.to_owned(),
		),
		(
			feature_name("ss01", 1, 0, 0, string).as_fea(),
			mac.to_owned(),
		),
		(
			feature_name("size", 3, 1, 0x409, string).as_fea(),
			format!("sizemenu{win}"),
		),
		(stat_name(3, 1, 0x409, string).as_fea(), win.to_owned()),
		(stat_name(1, 0, 0, string).as_fea(), mac.to_owned()),
		(cv.as_fea(), win.to_owned()),
	];
	for (actual, expected) in cases {
		assert_eq!(actual, expected);
	}
}

#[test]
fn test_name_statements_keep_non_ascii() {
	let statement = feature_name("ss01", 3, 1, 0x411, "ローマン");
	assert_eq!(statement.as_fea(), "name 3 1 1041 \"ローマン\";");
}

#[test]
fn test_feature_names_roundtrip_quote_and_backslash() -> Result<()> {
	let fea = "feature ss01 {\n    featureNames {\n        name \"a\\0022b\\005cc\";\n        name 1 \"a\\22b\\5cc\";\n    };\n} ss01;\n";
	let doc = Parser::new(FeatureSource::text(fea), Vec::<String>::new())?.parse()?;
	let reparsed =
		Parser::new(FeatureSource::text(doc.as_fea()?), Vec::<String>::new())?.parse()?;
	let feature = reparsed.statements[0].statements().unwrap_or_default();
	let names = feature[0].statements().unwrap_or_default();
	let strings: Vec<&str> = names
		.iter()
		.filter_map(|n| match n {
			Statement::FeatureNameStatement(n) => Some(n.string.as_str()),
			_ => None,
		})
		.collect();
	assert_eq!(strings, vec!["a\"b\\c", "a\"b\\c"]);
	Ok(())
}

/// `Tests/feaLib/variableScalar_test.py::test_variable_scalar_repr`.
#[test]
fn test_variable_scalar_repr() {
	let mut scalar = VariableScalar::new();
	scalar.add_value(Vec::<(String, f64)>::new(), 20);
	scalar.add_value([("wght", 400.0)], 1);
	scalar.add_value([("wght", 500.5)], 4);
	scalar.add_value([("wght", 700.0)], 10);
	assert_eq!(
		scalar.to_string(),
		"(20 wght=400:1 wght=500.5:4 wght=700:10)"
	);
}
