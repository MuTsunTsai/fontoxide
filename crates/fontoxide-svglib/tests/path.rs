//! `Tests/svgLib/path/path_test.py`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_pens::recording_pen::PenOp;
use fontoxide_pens::{RecordingPen, Transform};
use fontoxide_svglib::SvgPath;

const SVG_DATA: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 20010904//EN"
 "http://www.w3.org/TR/2001/REC-SVG-20010904/DTD/svg10.dtd">
<svg version="1.0" xmlns="http://www.w3.org/2000/svg"
 width="1000.0" height="1000.0">
<path d="M 100 100 L 300 100 L 200 300 z"/>
<path d="M100,200 C100,100 250,100 250,200 S400,300 400,200"/>
</svg>
"#;

fn expected_pen_commands() -> Vec<PenOp> {
	vec![
		PenOp::MoveTo((100.0, 100.0)),
		PenOp::LineTo((300.0, 100.0)),
		PenOp::LineTo((200.0, 300.0)),
		PenOp::LineTo((100.0, 100.0)),
		PenOp::ClosePath,
		PenOp::MoveTo((100.0, 200.0)),
		PenOp::CurveTo(vec![(100.0, 100.0), (250.0, 100.0), (250.0, 200.0)]),
		PenOp::CurveTo(vec![(250.0, 300.0), (400.0, 300.0), (400.0, 200.0)]),
		PenOp::EndPath,
	]
}

fn tmp_dir(name: &str) -> PathBuf {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-svglib-{}-{name}-{}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	));
	std::fs::create_dir_all(&dir).expect("create temp dir");
	dir
}

#[test]
fn test_from_svg_file() {
	let dir = tmp_dir("file");
	let path = dir.join("test.svg");
	std::fs::write(&path, SVG_DATA).expect("write");
	let svg = SvgPath::new(&path, None).expect("valid svg");
	let _ = std::fs::remove_dir_all(dir);
	let mut pen = RecordingPen::new();
	svg.draw(&mut pen).expect("drawable");
	assert_eq!(pen.value, expected_pen_commands());
}

#[test]
fn test_fromstring() {
	let svg = SvgPath::from_bytes(SVG_DATA.as_bytes(), None).expect("valid svg");
	let mut pen = RecordingPen::new();
	svg.draw(&mut pen).expect("drawable");
	assert_eq!(pen.value, expected_pen_commands());
}

#[test]
fn test_xml_comment_ignored() {
	let svg_with_comment = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
  <!-- a comment -->
  <path d="M100 100"/>
</svg>"#;
	let svg = SvgPath::fromstring(svg_with_comment, None).expect("valid svg");
	let mut pen = RecordingPen::new();
	svg.draw(&mut pen).expect("drawable");
	assert_eq!(pen.value, [PenOp::MoveTo((100.0, 100.0)), PenOp::EndPath]);
}

#[test]
fn test_transform() {
	let svg = SvgPath::fromstring(
		SVG_DATA,
		Some(Transform::new(1.0, 0.0, 0.0, -1.0, 0.0, 1000.0)),
	)
	.expect("valid svg");
	let mut pen = RecordingPen::new();
	svg.draw(&mut pen).expect("drawable");
	assert_eq!(
		pen.value,
		[
			PenOp::MoveTo((100.0, 900.0)),
			PenOp::LineTo((300.0, 900.0)),
			PenOp::LineTo((200.0, 700.0)),
			PenOp::LineTo((100.0, 900.0)),
			PenOp::ClosePath,
			PenOp::MoveTo((100.0, 800.0)),
			PenOp::CurveTo(vec![(100.0, 900.0), (250.0, 900.0), (250.0, 800.0)]),
			PenOp::CurveTo(vec![(250.0, 700.0), (400.0, 700.0), (400.0, 800.0)]),
			PenOp::EndPath,
		]
	);
}

/// `Path.as_uri()`.
fn file_uri(path: &std::path::Path) -> String {
	let s = path.to_string_lossy().replace('\\', "/");
	if s.starts_with('/') {
		format!("file://{s}")
	} else {
		format!("file:///{s}")
	}
}

#[test]
fn test_no_external_entity_expansion() {
	let dir = tmp_dir("xxe");
	let secret = dir.join("secret.txt");
	std::fs::write(&secret, "s3cr3t").expect("write");
	let svg = dir.join("test.svg");
	std::fs::write(
		&svg,
		format!(
			"<?xml version=\"1.0\"?><!DOCTYPE svg [<!ENTITY xxe SYSTEM \"{}\">]><svg xmlns=\"http://www.w3.org/2000/svg\"><desc>&xxe;</desc><path d=\"M 0 0 L 1 1 z\"/></svg>",
			file_uri(&secret)
		),
	)
	.expect("write");
	let data = std::fs::read(&svg).expect("read");
	for result in [SvgPath::new(&svg, None), SvgPath::from_bytes(&data, None)] {
		match result {
			// the undefined entity is rejected outright
			Err(e) => assert!(!e.to_string().contains("s3cr3t"), "{e}"),
			Ok(svg) => {
				let root = svg.root.expect("root");
				assert!(
					root.iter()
						.all(|el| !el.text.as_deref().unwrap_or("").contains("s3cr3t"))
				);
			}
		}
	}
	let _ = std::fs::remove_dir_all(dir);
}
