//! Table implementations (`fontTools.ttLib.tables`).
//!
//! Modules are named after the table they implement; the fontTools module is noted
//! in each.

pub mod avar;
pub mod cff;
pub mod cmap;
pub mod colr;
pub mod cpal;
pub mod cvt;
pub mod ebdt;
pub mod eblc;
pub mod fpgm;
pub mod fvar;
pub mod gasp;
pub mod glyf;
pub mod gvar;
pub mod hdmx;
pub mod head;
pub mod hhea;
pub mod loca;
pub mod maxp;
pub mod metrics;
pub mod misc_tables;
pub mod name;
pub mod os2;
pub mod os2_ranges;
pub mod otl;
pub mod post;
pub mod sbix;
pub mod tt_program;
pub mod tuple_variation;

pub use avar::Avar;
pub use cff::{Cff, Cff2};
pub use cmap::{Cmap, CmapSubtable};
pub use colr::Colr;
pub use cpal::Cpal;
pub use cvt::Cvt;
pub use ebdt::{BitmapData, Cbdt, Ebdt};
pub use eblc::{BitmapLocator, Cblc, Eblc};
pub use fpgm::{Fpgm, Prep};
pub use fvar::Fvar;
pub use gasp::Gasp;
pub use glyf::Glyf;
pub use gvar::{Cvar, Gvar};
pub use hdmx::{Hdmx, Ltsh};
pub use head::Head;
pub use hhea::{Hhea, Vhea};
pub use loca::Loca;
pub use maxp::Maxp;
pub use metrics::{Hmtx, Vmtx};
pub use misc_tables::{Fftm, Ltag, Vorg};
pub use name::Name;
pub use os2::Os2;
pub use otl::{Base, Gdef, Gpos, Gsub, Hvar, Jstf, Math, Mvar, Stat, Vvar};
pub use post::Post;
pub use sbix::Sbix;
