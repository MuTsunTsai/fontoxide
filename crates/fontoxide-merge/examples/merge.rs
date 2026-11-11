//! The command line of `fontoxide_merge::main`, like `fonttools merge`, until there
//! is a `fontoxide` command.
//!
//! With `FONTOXIDE_TIMING` set, the time of merging and saving is printed.

fn main() -> std::process::ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	let start = std::time::Instant::now();
	let result = fontoxide_merge::main(&args);
	if std::env::var_os("FONTOXIDE_TIMING").is_some() {
		eprintln!("{} ms", start.elapsed().as_millis());
	}
	match result {
		Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
		Err(e) => {
			eprintln!("{e}");
			std::process::ExitCode::FAILURE
		}
	}
}
