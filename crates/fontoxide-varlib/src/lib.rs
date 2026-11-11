//! Port of `fontTools.varLib`: tools for variable fonts.
//!
//! | fontoxide      | fontTools                  |
//! |----------------|----------------------------|
//! | [`builder`]    | `fontTools.varLib.builder` |
//! | [`feature_vars`] | `fontTools.varLib.featureVars` |
//! | [`instancer`]  | `fontTools.varLib.instancer` |
//! | [`iup`]        | `fontTools.varLib.iup`     |
//! | [`models`]     | `fontTools.varLib.models`  |
//! | [`mutator`]    | `fontTools.varLib.mutator` |
//! | [`var_store`]  | `fontTools.varLib.varStore` |

#![forbid(unsafe_code)]

pub mod avar_builder;
pub mod builder;
pub mod feature_vars;
pub mod instancer;
pub mod iup;
pub mod models;
pub mod mutator;
pub mod var_store;
