//! `Tests/misc/symfont_test.py`.
//!
//! `fontTools.misc.symfont` derives Green-theorem pens with sympy at run time;
//! its `AreaPen` computes the same signed area as `fontTools.pens.areaPen`,
//! which [`AreaPen`] ports, so the tests run against that.
#![allow(non_snake_case)]

use fontoxide_misc::py::round_digits;
use fontoxide_pens::area_pen::AreaPen;
use fontoxide_pens::{Pen, Result};

fn draw1(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((254.0, 360.0))?;
	pen.line_to((771.0, 367.0))?;
	pen.curve_to(&[(800.0, 393.0), (808.0, 399.0), (819.0, 412.0)])?;
	pen.curve_to(&[(818.0, 388.0), (774.0, 138.0), (489.0, 145.0)])?;
	pen.curve_to(&[(188.0, 145.0), (200.0, 398.0), (200.0, 421.0)])?;
	pen.curve_to(&[(209.0, 409.0), (220.0, 394.0), (254.0, 360.0)])?;
	pen.close_path()
}

#[test]
fn test_PScontour_clockwise_line_first() -> Result<()> {
	let mut pen = AreaPen::new_area(None);
	draw1(&mut pen)?;
	assert_eq!(round_digits(pen.value(), 6), -104561.35);
	Ok(())
}

#[test]
fn test_openPaths() -> Result<()> {
	let mut pen = AreaPen::new_area(None);
	pen.move_to((0.0, 0.0))?;
	pen.end_path()?;
	assert_eq!(pen.value(), 0.0);

	pen.move_to((0.0, 0.0))?;
	pen.line_to((1.0, 0.0))?;
	// NotImplementedError
	assert!(pen.end_path().is_err());
	Ok(())
}
