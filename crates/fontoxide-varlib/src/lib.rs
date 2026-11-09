//! Port of `fontTools.varLib`: tools for variable fonts.
//!
//! | fontoxide      | fontTools                  |
//! |----------------|----------------------------|
//! | [`instancer`]  | `fontTools.varLib.instancer` |
//! | [`iup`]        | `fontTools.varLib.iup`     |
//! | [`models`]     | `fontTools.varLib.models`  |
//! | [`var_store`]  | `fontTools.varLib.varStore` |

#![forbid(unsafe_code)]

pub mod instancer;
pub mod iup;
pub mod models;
pub mod var_store;
