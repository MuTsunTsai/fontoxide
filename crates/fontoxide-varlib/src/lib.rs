//! Port of `fontTools.varLib`: tools for variable fonts.
//!
//! | fontoxide      | fontTools                  |
//! |----------------|----------------------------|
//! | [`build`]      | `fontTools.varLib` (`build`, `build_many`, `main`...) |
//! | [`avar`]       | `fontTools.varLib.avar` (`build`, `map`, `plan`, `unbuild`) |
//! | [`builder`]    | `fontTools.varLib.builder` |
//! | [`cff`]        | `fontTools.varLib.cff`     |
//! | [`errors`]     | `fontTools.varLib.errors` |
//! | [`feature_vars`] | `fontTools.varLib.featureVars` |
//! | [`instancer`]  | `fontTools.varLib.instancer` |
//! | [`hvar`]       | `fontTools.varLib.hvar`    |
//! | [`interpolatable`] | `fontTools.varLib.interpolatable` (without the plots) |
//! | [`interpolate_layout`] | `fontTools.varLib.interpolate_layout` |
//! | [`iup`]        | `fontTools.varLib.iup`     |
//! | [`merger`]     | `fontTools.varLib.merger`  |
//! | [`models`]     | `fontTools.varLib.models`  |
//! | [`mutator`]    | `fontTools.varLib.mutator` |
//! | [`mvar`]       | `fontTools.varLib.mvar`    |
//! | [`stat`]       | `fontTools.varLib.stat`    |
//! | [`var_store`]  | `fontTools.varLib.varStore` |

#![forbid(unsafe_code)]

pub use build::{
	BuildOptions, DesignSpaceData, MasterFinder, SourceFonts,
	add_gsub_feature_variations_from_designspace, build, build_many, drop_implied_oncurve_points,
	load_designspace, load_masters, set_default_weight_width_slant,
};
pub use errors::{MergeError, VarLibError};

pub mod avar;
pub mod avar_builder;
pub mod build;
pub mod builder;
pub mod cff;
pub mod errors;
pub mod feature_vars;
pub mod hvar;
pub mod instancer;
pub mod interpolatable;
pub mod interpolate_layout;
pub mod iup;
pub mod merger;
pub mod models;
pub mod mutator;
pub mod mvar;
pub mod stat;
pub mod var_store;
