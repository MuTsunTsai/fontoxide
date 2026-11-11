//! The fontTools command lines.
//!
//! | fontoxide-cli    | fontTools            |
//! |------------------|----------------------|
//! | [`ttx`]          | `fontTools.ttx`      |
//! | [`fonttools_main`], [`run_module`], [`help`] | `fontTools.__main__`, `fontTools.help` |
//! | [`svg_path_pen`] | `fontTools.pens.svgPathPen.main` |
//! | [`logging`]      | `fontTools.misc.loggingTools.configLogger` |
//! | [`getopt`]       | Python's `getopt.gnu_getopt` |
//!
//! The binaries `fonttools`, `ttx`, `pyftsubset` and `pyftmerge` are fontTools'
//! console scripts. `fonttools <module> args...` runs the command line of a
//! module, as `python -m fontTools.<module> args...` does; [`MODULES`] lists
//! the modules that fontoxide has.
//!
//! The command lines themselves are in the crates of their modules, except for
//! `ttx`; they read and write files with `std::fs`.

#![forbid(unsafe_code)]

pub mod getopt;
pub mod logging;
pub mod svg_path_pen;
pub mod ttx;

use std::io::Write;

/// The error of a command line.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The `main` of a module: takes the arguments, returns the exit code.
pub type ModuleMain = fn(&[&str]) -> Result<i32, BoxError>;

/// A module with a command line.
#[derive(Debug, Clone, Copy)]
pub struct Module {
	/// The module name, without `fontTools.`.
	pub name: &'static str,
	/// The first line of the docstring of its `main`, which `fonttools help`
	/// shows; `None` hides the module there, as fontTools does when `main` has
	/// no docstring.
	pub description: Option<&'static str>,
	/// The command line.
	pub main: ModuleMain,
}

/// The modules that `fonttools` can run, sorted by name.
pub const MODULES: &[Module] = &[
	Module {
		name: "cffLib.CFFToCFF2",
		description: Some("Convert CFF OTF font to CFF2 OTF font"),
		main: |a| Ok(fontoxide_ttlib::cff::cff_to_cff2::main(a)?),
	},
	Module {
		name: "cu2qu",
		description: Some("Convert a UFO font from cubic to quadratic curves"),
		main: |a| Ok(fontoxide_cu2qu::cli::main(a)?),
	},
	Module {
		name: "feaLib",
		description: Some("Add features from a feature file (.fea) into an OTF font"),
		main: |a| Ok(fontoxide_fealib::cli::main(a)?),
	},
	Module {
		name: "help",
		description: Some("Show this help"),
		main: |_| {
			help(&mut std::io::stderr())?;
			Ok(0)
		},
	},
	Module {
		name: "merge",
		description: Some("Merge multiple fonts into one"),
		main: |a| Ok(fontoxide_merge::main(a)?),
	},
	Module {
		name: "mtiLib",
		description: Some("Convert a FontDame OTL file to TTX XML"),
		main: |a| Ok(fontoxide_mtilib::cli::main(a)?),
	},
	Module {
		name: "otlLib.optimize",
		description: Some("Optimize the layout tables of an existing font"),
		main: |a| Ok(fontoxide_otllib::optimize::main(a)?),
	},
	Module {
		name: "pens.svgPathPen",
		description: Some("Generate per-character SVG from font and text"),
		main: |a| Ok(svg_path_pen::main(a)?),
	},
	Module {
		name: "qu2cu",
		description: Some("Convert an OpenType font from quadratic to cubic curves"),
		main: |a| Ok(fontoxide_qu2cu::cli::main(a)?),
	},
	Module {
		name: "subset",
		description: Some("OpenType font subsetter and optimizer"),
		main: |a| Ok(fontoxide_subset::main(a)?),
	},
	Module {
		name: "ttLib",
		description: Some("Open/save fonts with TTFont() or TTCollection()"),
		main: |a| Ok(fontoxide_ttlib::cli::main(a)?),
	},
	Module {
		name: "ttLib.beyond64k",
		description: None,
		main: |a| Ok(fontoxide_ttlib::beyond64k::main(a)?),
	},
	Module {
		name: "ttLib.woff2",
		description: Some("Compress and decompress WOFF2 fonts"),
		main: |a| Ok(fontoxide_ttlib::woff2_cli::main(a)?),
	},
	Module {
		name: "ttx",
		description: Some("Convert OpenType fonts to XML and back"),
		main: |a| Ok(ttx::main(a)?),
	},
	Module {
		name: "varLib",
		description: Some("Build variable fonts from a designspace file and masters"),
		main: |a| Ok(fontoxide_varlib::build::main(a)?),
	},
	Module {
		name: "varLib.avar.map",
		description: Some("Map variation coordinates through the `avar` table."),
		main: |a| {
			let lines = fontoxide_varlib::avar::map_main(a)?;
			let mut out = std::io::stdout().lock();
			for line in lines {
				writeln!(out, "{line}")?;
			}
			Ok(0)
		},
	},
	Module {
		name: "varLib.instancer",
		description: Some("Partially instantiate a variable font"),
		main: |a| Ok(fontoxide_varlib::instancer::cli::main(a)?),
	},
	Module {
		name: "varLib.interpolatable",
		description: Some("Test for interpolatability issues between fonts"),
		main: |a| Ok(fontoxide_varlib::interpolatable::main(a)?),
	},
	Module {
		name: "varLib.interpolate_layout",
		description: Some("Interpolate GDEF/GPOS/GSUB tables for a point on a designspace"),
		main: |a| Ok(fontoxide_varlib::interpolate_layout::main(a)?),
	},
	Module {
		name: "varLib.mutator",
		description: Some("Instantiate a variation font"),
		main: |a| Ok(fontoxide_varlib::mutator::main(a)?),
	},
	Module {
		name: "voltLib",
		description: Some("Build tables from a MS VOLT project into an OTF font"),
		main: |a| Ok(fontoxide_voltlib::cli::main(a)?),
	},
	Module {
		name: "voltLib.voltToFea",
		description: Some("Convert MS VOLT to AFDKO feature files."),
		main: |a| Ok(fontoxide_voltlib::volt_to_fea::main(a)?),
	},
];

/// The command lines of fontTools that fontoxide does not have (yet), with the
/// descriptions `fonttools help` shows for them.
pub const MISSING_MODULES: &[(&str, &str)] = &[
	("cffLib.CFF2ToCFF", "Convert CFF2 OTF font to CFF OTF font"),
	(
		"cffLib.width",
		"Calculate optimum defaultWidthX/nominalWidthX values",
	),
	(
		"designspaceLib",
		"Roundtrip .designspace file through the DesignSpaceDocument class",
	),
	("diff", "Compare two fonts for differences"),
	(
		"pens.statisticsPen",
		"Report font glyph shape geometricsl statistics",
	),
	(
		"ttLib.removeOverlaps",
		"Simplify glyphs in TTFont by merging overlapping contours.",
	),
	("ttLib.scaleUpem", "Change the units-per-EM of fonts"),
	(
		"varLib.avar.build",
		"Add `avar` table from designspace file to variable font.",
	),
	(
		"varLib.avar.plan",
		"Plan the standard axis mappings for a variable font",
	),
	(
		"varLib.avar.unbuild",
		"Print `avar` table as a designspace snippet.",
	),
	("varLib.hvar", "Add `HVAR` table to variable font."),
	(
		"varLib.models",
		"Normalize locations on a given designspace",
	),
	("varLib.varStore", "Optimize a font's GDEF variation store"),
];

/// The module named `name` (without `fontTools.`).
pub fn find_module(name: &str) -> Option<&'static Module> {
	MODULES.iter().find(|m| m.name == name)
}

/// `fonttools help` (`fontTools.help`): the version and the modules with
/// their descriptions. fontTools lists all of its modules; this lists those
/// in [`MODULES`].
pub fn help(out: &mut dyn Write) -> std::io::Result<()> {
	let mut text = format!("fonttools v{}\n\n", fontoxide_ttlib::FONTTOOLS_VERSION);
	for module in MODULES {
		if let Some(description) = module.description {
			text.push_str(&format!("fonttools {:<25} {description}\n", module.name));
		}
	}
	out.write_all(text.as_bytes())
}

/// Runs the command line of a module (`runpy.run_module("fontTools." + name)`),
/// reporting errors on standard error. Returns the exit code.
pub fn run_module(name: &str, args: &[&str]) -> i32 {
	let Some(module) = find_module(name) else {
		let message = if MISSING_MODULES.iter().any(|(m, _)| *m == name) {
			format!("ERROR: fontoxide does not have the command line of fontTools.{name} yet")
		} else {
			format!("ERROR: No module named fontTools.{name}")
		};
		eprintln!("{message}");
		return 1;
	};
	match (module.main)(args) {
		Ok(code) => code,
		Err(e) => {
			eprintln!("ERROR: {e}");
			1
		}
	}
}

/// The command line `fonttools <module> args...` (`fontTools.__main__.main`):
/// runs a module's command line; without a module, or with `-h` or `--help`,
/// shows the help. Returns the exit code.
pub fn fonttools_main(args: &[&str]) -> i32 {
	match args.split_first() {
		None => run_module("help", &[]),
		Some((&("-h" | "--help"), rest)) => run_module("help", rest),
		Some((name, rest)) => run_module(name, rest),
	}
}

/// The arguments of the process, for the binaries (lossily converted to UTF-8).
pub fn process_args() -> Vec<String> {
	std::env::args_os()
		.skip(1)
		.map(|a| a.to_string_lossy().into_owned())
		.collect()
}
