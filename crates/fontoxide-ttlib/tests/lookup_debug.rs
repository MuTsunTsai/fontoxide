//! The feaLib debugging comments of `LookupList.toXML2`: a `Debg` table with
//! lookup debugging information makes the TTX dump of a `LookupList` write a
//! comment before each lookup that has an entry. The expected TTX is what
//! fontTools writes for the same font.

use fontoxide_ttlib::{Font, TableKey, Tag, TtxOptions};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// A font with a `GSUB` table of two lookups and a `Debg` table with an entry
/// for the second; `DEBG_GSUB` is the JSON object for `GSUB`.
const FONT_TTX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ttFont sfntVersion="\x00\x01\x00\x00" ttLibVersion="4.68">

  <GlyphOrder>
    <GlyphID id="0" name=".notdef"/>
    <GlyphID id="1" name="a"/>
    <GlyphID id="2" name="b"/>
  </GlyphOrder>

  <GSUB>
    <Version value="0x00010000"/>
    <ScriptList>
      <ScriptRecord index="0">
        <ScriptTag value="DFLT"/>
        <Script>
          <DefaultLangSys>
            <ReqFeatureIndex value="65535"/>
            <FeatureIndex index="0" value="0"/>
          </DefaultLangSys>
        </Script>
      </ScriptRecord>
    </ScriptList>
    <FeatureList>
      <FeatureRecord index="0">
        <FeatureTag value="test"/>
        <Feature>
          <LookupListIndex index="0" value="0"/>
          <LookupListIndex index="1" value="1"/>
        </Feature>
      </FeatureRecord>
    </FeatureList>
    <LookupList>
      <Lookup index="0">
        <LookupType value="1"/>
        <LookupFlag value="0"/>
        <SingleSubst index="0">
          <Substitution in="a" out="b"/>
        </SingleSubst>
      </Lookup>
      <Lookup index="1">
        <LookupType value="1"/>
        <LookupFlag value="0"/>
        <SingleSubst index="0">
          <Substitution in="b" out="a"/>
        </SingleSubst>
      </Lookup>
    </LookupList>
  </GSUB>

  <Debg>
    <json>
      <![CDATA[{"com.github.fonttools.feaLib": {DEBG_GSUB "GPOS": {}}}]]>
    </json>
  </Debg>

</ttFont>
"#;

/// The `GSUB` part of the `Debg` data: one entry, for lookup 1.
const DEBG_ONE_ENTRY: &str =
	r#""GSUB": {"1": ["features.fea:7:3", "second", ["DFLT", "dflt", "test"]]},"#;

/// fontTools' `saveXML(tables=["GSUB"])` of the font with `DEBG_ONE_ENTRY`.
const EXPECTED_ONE_ENTRY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ttFont sfntVersion="\x00\x01\x00\x00" ttLibVersion="4.68">

  <GSUB>
    <Version value="0x00010000"/>
    <ScriptList>
      <!-- ScriptCount=1 -->
      <ScriptRecord index="0">
        <ScriptTag value="DFLT"/>
        <Script>
          <DefaultLangSys>
            <ReqFeatureIndex value="65535"/>
            <!-- FeatureCount=1 -->
            <FeatureIndex index="0" value="0"/>
          </DefaultLangSys>
          <!-- LangSysCount=0 -->
        </Script>
      </ScriptRecord>
    </ScriptList>
    <FeatureList>
      <!-- FeatureCount=1 -->
      <FeatureRecord index="0">
        <FeatureTag value="test"/>
        <Feature>
          <!-- LookupCount=2 -->
          <LookupListIndex index="0" value="0"/>
          <LookupListIndex index="1" value="1"/>
        </Feature>
      </FeatureRecord>
    </FeatureList>
    <LookupList>
      <!-- LookupCount=2 -->
      <Lookup index="0">
        <LookupType value="1"/>
        <LookupFlag value="0"/>
        <!-- SubTableCount=1 -->
        <SingleSubst index="0">
          <Substitution in="a" out="b"/>
        </SingleSubst>
      </Lookup>
      <!-- second: features.fea:7:3 in test (DFLT/dflt) -->
      <Lookup index="1">
        <LookupType value="1"/>
        <LookupFlag value="0"/>
        <!-- SubTableCount=1 -->
        <SingleSubst index="0">
          <Substitution in="b" out="a"/>
        </SingleSubst>
      </Lookup>
    </LookupList>
  </GSUB>

</ttFont>
"#;

fn font_with_debg(debg_gsub: &str) -> std::result::Result<Font, Box<dyn std::error::Error>> {
	Ok(Font::from_ttx(&FONT_TTX.replace("DEBG_GSUB ", debg_gsub))?)
}

fn gsub_ttx(font: &mut Font) -> fontoxide_ttlib::Result<String> {
	font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(Tag::new(b"GSUB"))],
		newline: "\n".into(),
		..Default::default()
	})
}

#[test]
fn test_lookup_debug_comment() -> TestResult {
	let mut font = font_with_debg(DEBG_ONE_ENTRY)?;
	assert_eq!(gsub_ttx(&mut font)?, EXPECTED_ONE_ENTRY);
	Ok(())
}

#[test]
fn test_lookup_debug_comment_without_name_and_feature() -> TestResult {
	// A falsy name and feature leave only the (escaped) location, as fontTools
	// writes it.
	let debg = DEBG_ONE_ENTRY.replace(
		r#""GSUB": {"#,
		r#""GSUB": {"0": ["<features>:2:1", null, []], "#,
	);
	let mut font = font_with_debg(&debg)?;
	let expected = EXPECTED_ONE_ENTRY.replace(
		"      <Lookup index=\"0\">\n",
		"      <!-- &lt;features&gt;:2:1 -->\n      <Lookup index=\"0\">\n",
	);
	assert_eq!(gsub_ttx(&mut font)?, expected);
	Ok(())
}

#[test]
fn test_lookup_debug_without_fealib_data() -> TestResult {
	// Without the feaLib key, the LookupList is written as usual.
	let ttx = FONT_TTX.replace("com.github.fonttools.feaLib", "other");
	let mut font = Font::from_ttx(&ttx.replace("DEBG_GSUB ", DEBG_ONE_ENTRY))?;
	let expected = EXPECTED_ONE_ENTRY.replace(
		"      <!-- second: features.fea:7:3 in test (DFLT/dflt) -->\n",
		"",
	);
	assert_eq!(gsub_ttx(&mut font)?, expected);
	Ok(())
}

#[test]
fn test_lookup_debug_missing_table_data() -> TestResult {
	// fontTools indexes the feaLib data with the LookupList's table, and raises
	// KeyError if it is missing.
	let mut font = font_with_debg("")?;
	let err = gsub_ttx(&mut font).expect_err("KeyError");
	assert!(err.to_string().contains("KeyError: 'GSUB'"), "{err}");
	Ok(())
}
