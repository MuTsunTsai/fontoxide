//! The command line of `fontoxide_varlib::instancer::cli::main`, like
//! `fonttools varLib.instancer`, until there is a `fontoxide` command.

fn main() -> std::process::ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	match fontoxide_varlib::instancer::cli::main(&args) {
		Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
		Err(e) => {
			eprintln!("{e}");
			std::process::ExitCode::FAILURE
		}
	}
}
