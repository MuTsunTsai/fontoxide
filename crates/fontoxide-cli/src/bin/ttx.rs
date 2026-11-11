//! The console script `ttx` of fontTools (`fontTools.ttx:main`).

fn main() -> std::process::ExitCode {
	fontoxide_cli::logging::config_logger();
	let args = fontoxide_cli::process_args();
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	let code = fontoxide_cli::run_module("ttx", &args);
	std::process::ExitCode::from(u8::try_from(code).unwrap_or(1))
}
