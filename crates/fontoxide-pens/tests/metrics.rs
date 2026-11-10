//! Ports of fontTools' `Tests/pens/areaPen_test.py`,
//! `Tests/pens/perimeterPen_test.py` and `Tests/pens/boundsPen_test.py`.

use fontoxide_misc::py::round_digits;
use fontoxide_pens::area_pen::AreaPen;
use fontoxide_pens::perimeter_pen::PerimeterPen;
use fontoxide_pens::{BoundsPen, ControlBoundsPen, Error, Pen, Point, Result};

type Draw = fn(&mut dyn Pen) -> Result<()>;

fn curves(pen: &mut dyn Pen, curves: &[[Point; 3]]) -> Result<()> {
	curves.iter().try_for_each(|c| pen.curve_to(c))
}

fn quads(pen: &mut dyn Pen, quads: &[[Point; 2]]) -> Result<()> {
	quads
		.iter()
		.try_for_each(|q| pen.q_curve_to(&q[..1], Some(q[1])))
}

const PS_CW: [[Point; 3]; 4] = [
	[(800.0, 393.0), (808.0, 399.0), (819.0, 412.0)],
	[(818.0, 388.0), (774.0, 138.0), (489.0, 145.0)],
	[(188.0, 145.0), (200.0, 398.0), (200.0, 421.0)],
	[(209.0, 409.0), (220.0, 394.0), (254.0, 360.0)],
];
const PS_CCW: [[Point; 3]; 4] = [
	[(220.0, 394.0), (209.0, 409.0), (200.0, 421.0)],
	[(200.0, 398.0), (188.0, 145.0), (489.0, 145.0)],
	[(774.0, 138.0), (818.0, 388.0), (819.0, 412.0)],
	[(808.0, 399.0), (800.0, 393.0), (771.0, 367.0)],
];
const TT_CW: [[Point; 2]; 16] = [
	[(793.0, 386.0), (802.0, 394.0)],
	[(811.0, 402.0), (819.0, 412.0)],
	[(819.0, 406.0), (814.0, 383.5)],
	[(809.0, 361.0), (796.0, 330.5)],
	[(783.0, 300.0), (760.5, 266.5)],
	[(738.0, 233.0), (701.0, 205.5)],
	[(664.0, 178.0), (612.0, 160.5)],
	[(560.0, 143.0), (489.0, 145.0)],
	[(414.0, 145.0), (363.0, 164.0)],
	[(312.0, 183.0), (280.0, 211.5)],
	[(248.0, 240.0), (231.5, 274.5)],
	[(215.0, 309.0), (208.0, 339.5)],
	[(201.0, 370.0), (200.5, 392.5)],
	[(200.0, 415.0), (200.0, 421.0)],
	[(207.0, 412.0), (217.5, 399.0)],
	[(228.0, 386.0), (254.0, 360.0)],
];
const TT_CCW: [[Point; 2]; 16] = [
	[(228.0, 386.0), (217.5, 399.0)],
	[(207.0, 412.0), (200.0, 421.0)],
	[(200.0, 415.0), (200.5, 392.5)],
	[(201.0, 370.0), (208.0, 339.5)],
	[(215.0, 309.0), (231.5, 274.5)],
	[(248.0, 240.0), (280.0, 211.5)],
	[(312.0, 183.0), (363.0, 164.0)],
	[(414.0, 145.0), (489.0, 145.0)],
	[(560.0, 143.0), (612.0, 160.5)],
	[(664.0, 178.0), (701.0, 205.5)],
	[(738.0, 233.0), (760.5, 266.5)],
	[(783.0, 300.0), (796.0, 330.5)],
	[(809.0, 361.0), (814.0, 383.5)],
	[(819.0, 406.0), (819.0, 412.0)],
	[(811.0, 402.0), (802.0, 394.0)],
	[(793.0, 386.0), (771.0, 367.0)],
];

fn draw1(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((254.0, 360.0))?;
	pen.line_to((771.0, 367.0))?;
	curves(pen, &PS_CW)?;
	pen.close_path()
}
fn draw2(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((254.0, 360.0))?;
	curves(pen, &PS_CCW)?;
	pen.close_path()
}
fn draw3(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((771.0, 367.0))?;
	curves(pen, &PS_CW)?;
	pen.close_path()
}
fn draw4(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((771.0, 367.0))?;
	pen.line_to((254.0, 360.0))?;
	curves(pen, &PS_CCW)?;
	pen.close_path()
}
fn draw5(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((254.0, 360.0))?;
	pen.line_to((771.0, 367.0))?;
	quads(pen, &TT_CW)?;
	pen.close_path()
}
fn draw6(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((254.0, 360.0))?;
	quads(pen, &TT_CCW)?;
	pen.close_path()
}
fn draw7(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((771.0, 367.0))?;
	quads(pen, &TT_CW)?;
	pen.close_path()
}
fn draw8(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((771.0, 367.0))?;
	pen.line_to((254.0, 360.0))?;
	quads(pen, &TT_CCW)?;
	pen.close_path()
}

const DRAWINGS: [Draw; 8] = [draw1, draw2, draw3, draw4, draw5, draw6, draw7, draw8];

/// `AreaPenTest`: the eight contours, clockwise or not, starting with a line or
/// not, cubic or quadratic.
#[test]
fn test_area() -> Result<()> {
	let expected = [
		-104561.35,
		104561.35,
		-104561.35,
		104561.35,
		-104602.791667,
		104602.791667,
		-104602.791667,
		104602.791667,
	];
	for (draw, e) in DRAWINGS.iter().zip(expected) {
		let mut pen = AreaPen::new_area(None);
		draw(&mut pen)?;
		assert_eq!(round_digits(pen.value(), 6), e);
	}
	Ok(())
}

#[test]
fn test_area_open_paths() -> Result<()> {
	let mut pen = AreaPen::new_area(None);
	pen.move_to((0.0, 0.0))?;
	pen.end_path()?;
	assert_eq!(pen.value(), 0.0);
	pen.move_to((0.0, 0.0))?;
	pen.line_to((1.0, 0.0))?;
	assert!(matches!(pen.end_path(), Err(Error::Usage(m)) if m == "NotImplementedError"));
	Ok(())
}

/// `PerimeterPenTest`.
#[test]
fn test_perimeter() -> Result<()> {
	for draw in DRAWINGS {
		let mut pen = PerimeterPen::new_perimeter(None, 0.005);
		draw(&mut pen)?;
		assert_eq!(pen.value().round_ties_even(), 1589.0);
	}
	Ok(())
}

fn draw_bounds(pen: &mut dyn Pen) -> Result<()> {
	pen.move_to((0.0, 0.0))?;
	pen.line_to((0.0, 100.0))?;
	pen.q_curve_to(
		&[(50.0, 75.0), (60.0, 50.0), (50.0, 25.0)],
		Some((0.0, 0.0)),
	)?;
	pen.curve_to(&[(-50.0, 25.0), (-60.0, 50.0), (-50.0, 75.0), (0.0, 100.0)])?;
	pen.close_path()
}

fn bounds_str(b: Option<[f64; 4]>) -> Option<String> {
	b.map(|b| {
		let v: Vec<String> = b.iter().map(|c| format!("{c:.0}")).collect();
		v.join(" ")
	})
}

#[test]
fn test_bounds_pen() -> Result<()> {
	let mut pen = BoundsPen::new_bounds(None);
	draw_bounds(&mut pen)?;
	assert_eq!(bounds_str(pen.bounds()).as_deref(), Some("-55 0 58 100"));

	assert_eq!(BoundsPen::new_bounds(None).bounds(), None);

	let mut pen = BoundsPen::new_bounds(None);
	pen.move_to((0.0, 0.0))?;
	pen.curve_to(&[(20.0, 10.0), (90.0, 40.0), (0.0, 0.0)])?;
	assert_eq!(bounds_str(pen.bounds()).as_deref(), Some("0 0 45 20"));

	let mut pen = BoundsPen::new_bounds(None);
	pen.move_to((0.0, 0.0))?;
	pen.q_curve_to(&[(6.0, 6.0)], Some((10.0, 0.0)))?;
	assert_eq!(bounds_str(pen.bounds()).as_deref(), Some("0 0 10 3"));
	Ok(())
}

#[test]
fn test_control_bounds_pen() -> Result<()> {
	let mut pen = ControlBoundsPen::new_control_bounds(None, false);
	draw_bounds(&mut pen)?;
	assert_eq!(bounds_str(pen.bounds()).as_deref(), Some("-55 0 60 100"));

	assert_eq!(
		ControlBoundsPen::new_control_bounds(None, false).bounds(),
		None
	);

	let mut pen = ControlBoundsPen::new_control_bounds(None, false);
	pen.move_to((0.0, 0.0))?;
	pen.curve_to(&[(20.0, 10.0), (90.0, 40.0), (0.0, 0.0)])?;
	assert_eq!(bounds_str(pen.bounds()).as_deref(), Some("0 0 90 40"));

	let mut pen = ControlBoundsPen::new_control_bounds(None, false);
	pen.move_to((0.0, 0.0))?;
	pen.q_curve_to(&[(6.0, 6.0)], Some((10.0, 0.0)))?;
	assert_eq!(bounds_str(pen.bounds()).as_deref(), Some("0 0 10 6"));

	let mut pen = ControlBoundsPen::new_control_bounds(None, false);
	pen.move_to((-5.0, 10.0))?;
	assert_eq!(bounds_str(pen.bounds()).as_deref(), Some("-5 10 -5 10"));

	let mut pen = ControlBoundsPen::new_control_bounds(None, true);
	pen.move_to((0.0, 10.0))?;
	assert_eq!(pen.bounds(), None);
	Ok(())
}
