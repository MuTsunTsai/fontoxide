//! Table implementations (`fontTools.ttLib.tables`).
//!
//! Modules are named after the table they implement; the fontTools module is noted
//! in each.

pub mod cff_names;
pub mod cmap;
pub mod cvt;
pub mod fvar;
pub mod gasp;
pub mod hdmx;
pub mod head;
pub mod hhea;
pub mod maxp;
pub mod metrics;
pub mod misc_tables;
pub mod name;
pub mod os2;
pub mod otl;
pub mod post;

pub use cmap::{Cmap, CmapSubtable};
pub use cvt::Cvt;
pub use fvar::Fvar;
pub use gasp::Gasp;
pub use hdmx::{Hdmx, Ltsh};
pub use head::Head;
pub use hhea::{Hhea, Vhea};
pub use maxp::Maxp;
pub use metrics::{Hmtx, Vmtx};
pub use misc_tables::{Fftm, Ltag, Vorg};
pub use name::Name;
pub use os2::Os2;
pub use otl::{Base, Gdef, Gpos, Gsub, Hvar, Jstf, Math, Mvar, Stat, Vvar};
pub use post::Post;
