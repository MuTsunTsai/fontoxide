//! The fontTools command lines.
//!
//! | fontoxide-cli    | fontTools            |
//! |------------------|----------------------|
//! | [`ttx`]          | `fontTools.ttx`      |
//! | [`fontoxide_main`], [`fonttools_main`], [`run_module`], [`help`] | `fontTools.__main__`, `fontTools.help` |
//! | [`statistics_pen`] | `fontTools.pens.statisticsPen.main` |
//! | [`svg_path_pen`] | `fontTools.pens.svgPathPen.main` |
//! | [`logging`]      | `fontTools.misc.loggingTools.configLogger` |
//! | [`getopt`]       | Python's `getopt.gnu_getopt` |
//!
//! The binary `fontoxide` runs the command lines: `fontoxide <module> args...`
//! runs the command line of a module, as `python -m fontTools.<module> args...`
//! and fontTools' `fonttools <module> args...` do ([`fontoxide_main`]);
//! [`MODULES`] lists the modules that fontoxide has. Its usage and help texts
//! name the program `fontoxide <module>` ([`Program::Fontoxide`]).
//!
//! With the feature `fonttools-names`, the binaries `fonttools`, `ttx`,
//! `pyftsubset` and `pyftmerge` are installed too: fontTools' console scripts,
//! with fontTools' texts ([`fonttools_main`], [`run_module`]).
//!
//! The command lines themselves are in the crates of their modules, except for
//! `ttx`; they read and write files with `std::fs`.

#![forbid(unsafe_code)]

pub mod getopt;
pub mod logging;
pub mod statistics_pen;
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
		name: "cffLib.CFF2ToCFF",
		description: Some("Convert CFF2 OTF font to CFF OTF font"),
		main: |a| Ok(fontoxide_ttlib::cff::cff2_to_cff::main(a)?),
	},
	Module {
		name: "cffLib.CFFToCFF2",
		description: Some("Convert CFF OTF font to CFF2 OTF font"),
		main: |a| Ok(fontoxide_ttlib::cff::cff_to_cff2::main(a)?),
	},
	Module {
		name: "cffLib.width",
		description: Some("Calculate optimum defaultWidthX/nominalWidthX values"),
		main: |a| {
			// Without arguments, the module runs its doctests.
			if a.is_empty() {
				return Ok(0);
			}
			Ok(fontoxide_ttlib::cff::width::main(a)?)
		},
	},
	Module {
		name: "cu2qu",
		description: Some("Convert a UFO font from cubic to quadratic curves"),
		main: |a| Ok(fontoxide_cu2qu::cli::main(a)?),
	},
	Module {
		name: "designspaceLib",
		description: Some("Roundtrip .designspace file through the DesignSpaceDocument class"),
		main: |a| Ok(fontoxide_designspacelib::cli::main(a)?),
	},
	Module {
		name: "diff",
		description: Some("Compare two fonts for differences"),
		main: |a| Ok(fontoxide_diff::main(a)?),
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
		name: "pens.statisticsPen",
		description: Some("Report font glyph shape geometricsl statistics"),
		main: |a| Ok(statistics_pen::main(a)?),
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
		name: "ttLib.removeOverlaps",
		description: Some("Simplify glyphs in TTFont by merging overlapping contours."),
		main: |a| Ok(fontoxide_ttlib::remove_overlaps::main(a)?),
	},
	Module {
		name: "ttLib.scaleUpem",
		description: Some("Change the units-per-EM of fonts"),
		main: |a| Ok(fontoxide_ttlib::scale_upem::main(a)?),
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
		name: "varLib.avar.build",
		description: Some("Add `avar` table from designspace file to variable font."),
		main: |a| Ok(fontoxide_varlib::avar::cli::build_main(a)?),
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
		name: "varLib.avar.plan",
		description: Some("Plan the standard axis mappings for a variable font"),
		main: |a| Ok(fontoxide_varlib::avar::plan::main(a)?),
	},
	Module {
		name: "varLib.avar.unbuild",
		description: Some("Print `avar` table as a designspace snippet."),
		main: |a| Ok(fontoxide_varlib::avar::cli::unbuild_main(a)?),
	},
	Module {
		name: "varLib.avarPlanner",
		description: None,
		main: |a| Ok(fontoxide_varlib::avar::plan::avar_planner_main(a)?),
	},
	Module {
		name: "varLib.hvar",
		description: Some("Add `HVAR` table to variable font."),
		main: |a| Ok(fontoxide_varlib::hvar::main(a)?),
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
		name: "varLib.models",
		description: Some("Normalize locations on a given designspace"),
		main: |a| {
			// Without arguments, the module runs its doctests.
			if a.is_empty() {
				return Ok(0);
			}
			Ok(fontoxide_varlib::models::main(a)?)
		},
	},
	Module {
		name: "varLib.mutator",
		description: Some("Instantiate a variation font"),
		main: |a| Ok(fontoxide_varlib::mutator::main(a)?),
	},
	Module {
		name: "varLib.varStore",
		description: Some("Optimize a font's GDEF variation store"),
		main: |a| {
			// Without arguments, the module runs its doctests.
			if a.is_empty() {
				return Ok(0);
			}
			Ok(fontoxide_varlib::var_store::main(a)?)
		},
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
pub const MISSING_MODULES: &[(&str, &str)] = &[];

/// The module named `name` (without `fontTools.`).
pub fn find_module(name: &str) -> Option<&'static Module> {
	MODULES.iter().find(|m| m.name == name)
}

/// The name of the `fontoxide` executable.
pub const FONTOXIDE: &str = "fontoxide";

/// The name a command line runs under, which its usage and help texts show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Program {
	/// fontTools' console scripts (`fonttools <module>`, `ttx`, `pyftsubset`,
	/// `pyftmerge`): the texts are fontTools'.
	#[default]
	FontTools,
	/// The `fontoxide` executable: the texts name `fontoxide <module>` where
	/// fontTools names `fonttools <module>`, `ttx`, or an `argparse` `prog`
	/// such as `cu2qu` (see [`fontoxide_misc::cli_tools::program_text`]).
	Fontoxide,
}

impl Program {
	/// Runs `f` with the command lines naming themselves as this program.
	pub fn run<R>(self, f: impl FnOnce() -> R) -> R {
		match self {
			Program::FontTools => f(),
			Program::Fontoxide => fontoxide_misc::cli_tools::with_program_name(FONTOXIDE, f),
		}
	}
}

/// The version that `fontoxide --version` prints, e.g.
/// `fontoxide 0.1.0 (fontTools 4.68.0)`.
pub fn version() -> String {
	format!(
		"{FONTOXIDE} {} (fontTools {})",
		env!("CARGO_PKG_VERSION"),
		fontoxide_ttlib::FONTTOOLS_VERSION
	)
}

/// `fonttools help` (`fontTools.help`): the version and the modules with
/// their descriptions. fontTools lists all of its modules; this lists those
/// in [`MODULES`].
///
/// Under the `fontoxide` executable (in [`run_module_as`] with
/// [`Program::Fontoxide`]), the first line is [`version`] and the modules are
/// listed as `fontoxide <module>`.
pub fn help(out: &mut dyn Write) -> std::io::Result<()> {
	let (program, header) = match fontoxide_misc::cli_tools::program_name() {
		None => (
			"fonttools",
			format!("fonttools v{}", fontoxide_ttlib::FONTTOOLS_VERSION),
		),
		Some(name) => (name, version()),
	};
	let mut text = format!("{header}\n\n");
	for module in MODULES {
		if let Some(description) = module.description {
			text.push_str(&format!("{program} {:<25} {description}\n", module.name));
		}
	}
	out.write_all(text.as_bytes())
}

/// Runs the command line of a module (`runpy.run_module("fontTools." + name)`),
/// reporting errors on standard error. Returns the exit code.
pub fn run_module(name: &str, args: &[&str]) -> i32 {
	run_module_as(Program::FontTools, name, args)
}

/// [`run_module`] under the name `program`, which the usage and help texts of
/// the module show.
pub fn run_module_as(program: Program, name: &str, args: &[&str]) -> i32 {
	program.run(|| run_module_inner(name, args))
}

fn run_module_inner(name: &str, args: &[&str]) -> i32 {
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
	dispatch(Program::FontTools, args)
}

/// The command line `fontoxide <module> args...`: [`fonttools_main`], with
/// the texts naming `fontoxide <module>` (see [`Program::Fontoxide`]); in
/// addition, `--version` and `-V` print [`version`]. Returns the exit code.
pub fn fontoxide_main(args: &[&str]) -> i32 {
	fontoxide_main_with_output(args, &mut std::io::stdout())
}

/// [`fontoxide_main`], with the output of `--version` written to `out`.
pub fn fontoxide_main_with_output(args: &[&str], out: &mut dyn Write) -> i32 {
	if let Some(&("--version" | "-V")) = args.first() {
		return match writeln!(out, "{}", version()) {
			Ok(()) => 0,
			Err(e) => {
				eprintln!("ERROR: {e}");
				1
			}
		};
	}
	dispatch(Program::Fontoxide, args)
}

fn dispatch(program: Program, args: &[&str]) -> i32 {
	match args.split_first() {
		None => run_module_as(program, "help", &[]),
		Some((&("-h" | "--help"), rest)) => run_module_as(program, "help", rest),
		Some((name, rest)) => run_module_as(program, name, rest),
	}
}

/// The arguments of the process, for the binaries (lossily converted to UTF-8).
pub fn process_args() -> Vec<String> {
	std::env::args_os()
		.skip(1)
		.map(|a| a.to_string_lossy().into_owned())
		.collect()
}
