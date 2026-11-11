//! fontoxide's own tests (no upstream counterpart): the modules that
//! `fonttools <module>` runs.

use fontoxide_cli::{MISSING_MODULES, MODULES, fonttools_main};

#[test]
fn modules_are_sorted_and_not_missing() {
	for pair in MODULES.windows(2) {
		assert!(
			pair[0].name < pair[1].name,
			"{} {}",
			pair[0].name,
			pair[1].name
		);
	}
	for (name, _) in MISSING_MODULES {
		assert!(MODULES.iter().all(|m| m.name != *name), "{name}");
	}
}

/// `cffLib.width`, `varLib.models` and `varLib.varStore` run their doctests
/// without arguments (which pass silently), not their `main`.
#[test]
fn doctest_modules_without_arguments() {
	for name in ["cffLib.width", "varLib.models", "varLib.varStore"] {
		assert_eq!(fonttools_main(&[name]), 0, "{name}");
	}
	// The others report the missing arguments.
	assert_eq!(fonttools_main(&["ttLib.scaleUpem"]), 2);
	assert_eq!(fonttools_main(&["designspaceLib"]), 2);
}
