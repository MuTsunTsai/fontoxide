//! Port of Python's `getopt.gnu_getopt`, which `fontTools.ttx` parses its
//! command line with.

/// `getopt.GetoptError`: the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct GetoptError(pub String);

/// The parsed options, as `(option, value)` pairs (`-x` or `--name`, and `""`
/// for options without a value), and the remaining arguments.
pub type Parsed = (Vec<(String, String)>, Vec<String>);

/// `getopt.gnu_getopt(args, shortopts, longopts)`: options and other arguments
/// may be intermixed, unless `shortopts` starts with `+` or the environment
/// variable `POSIXLY_CORRECT` is set. A short option followed by `:` in
/// `shortopts`, and a long option ending with `=` in `longopts`, take a value.
/// Long options can be abbreviated to a unique prefix.
pub fn gnu_getopt(
	args: &[&str],
	shortopts: &str,
	longopts: &[&str],
) -> Result<Parsed, GetoptError> {
	let mut opts = Vec::new();
	let mut prog_args = Vec::new();
	let (shortopts, all_options_first) = match shortopts.strip_prefix('+') {
		Some(rest) => (rest, true),
		None => (
			shortopts,
			std::env::var_os("POSIXLY_CORRECT").is_some_and(|v| !v.is_empty()),
		),
	};
	let mut args = args.iter().copied();
	while let Some(arg) = args.next() {
		if arg == "--" {
			prog_args.extend(args.map(str::to_owned));
			break;
		}
		if let Some(long) = arg.strip_prefix("--") {
			do_longs(&mut opts, long, longopts, &mut args)?;
		} else if arg.starts_with('-') && arg != "-" {
			do_shorts(&mut opts, &arg[1..], shortopts, &mut args)?;
		} else if all_options_first {
			prog_args.push(arg.to_owned());
			prog_args.extend(args.map(str::to_owned));
			break;
		} else {
			prog_args.push(arg.to_owned());
		}
	}
	Ok((opts, prog_args))
}

fn do_longs<'a>(
	opts: &mut Vec<(String, String)>,
	opt: &str,
	longopts: &[&str],
	args: &mut impl Iterator<Item = &'a str>,
) -> Result<(), GetoptError> {
	let (opt, optarg) = match opt.split_once('=') {
		Some((opt, arg)) => (opt, Some(arg)),
		None => (opt, None),
	};
	let (has_arg, opt) = long_has_args(opt, longopts)?;
	let optarg = if has_arg {
		match optarg {
			Some(arg) => arg.to_owned(),
			None => args
				.next()
				.ok_or_else(|| GetoptError(format!("option --{opt} requires argument")))?
				.to_owned(),
		}
	} else if optarg.is_some() {
		return Err(GetoptError(format!(
			"option --{opt} must not have an argument"
		)));
	} else {
		String::new()
	};
	opts.push((format!("--{opt}"), optarg));
	Ok(())
}

fn long_has_args(opt: &str, longopts: &[&str]) -> Result<(bool, String), GetoptError> {
	let possibilities: Vec<&str> = longopts
		.iter()
		.copied()
		.filter(|o| o.starts_with(opt))
		.collect();
	if possibilities.is_empty() {
		return Err(GetoptError(format!("option --{opt} not recognized")));
	}
	// Is there an exact match?
	if possibilities.contains(&opt) {
		return Ok((false, opt.to_owned()));
	}
	if possibilities.contains(&format!("{opt}=").as_str()) {
		return Ok((true, opt.to_owned()));
	}
	// No exact match, so better be unique.
	if possibilities.len() > 1 {
		return Err(GetoptError(format!("option --{opt} not a unique prefix")));
	}
	let unique_match = possibilities[0];
	Ok(match unique_match.strip_suffix('=') {
		Some(name) => (true, name.to_owned()),
		None => (false, unique_match.to_owned()),
	})
}

fn do_shorts<'a>(
	opts: &mut Vec<(String, String)>,
	optstring: &str,
	shortopts: &str,
	args: &mut impl Iterator<Item = &'a str>,
) -> Result<(), GetoptError> {
	let mut rest = optstring;
	while let Some(opt) = rest.chars().next() {
		rest = &rest[opt.len_utf8()..];
		let optarg = if short_has_arg(opt, shortopts)? {
			let value = if rest.is_empty() {
				args.next()
					.ok_or_else(|| GetoptError(format!("option -{opt} requires argument")))?
			} else {
				rest
			};
			rest = "";
			value.to_owned()
		} else {
			String::new()
		};
		opts.push((format!("-{opt}"), optarg));
	}
	Ok(())
}

fn short_has_arg(opt: char, shortopts: &str) -> Result<bool, GetoptError> {
	let chars: Vec<char> = shortopts.chars().collect();
	for (i, &c) in chars.iter().enumerate() {
		if opt == c && c != ':' {
			return Ok(chars.get(i + 1) == Some(&':'));
		}
	}
	Err(GetoptError(format!("option -{opt} not recognized")))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
		list.iter()
			.map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
			.collect()
	}

	/// From CPython's `test_getopt.test_gnu_getopt`.
	#[test]
	fn gnu_getopt_cpython() -> Result<(), GetoptError> {
		let cmdline = [
			"-a", "arg1", "-b", "1", "--alpha", "--beta=2", "--beta", "3", "-", "-", "--", "arg2",
		];
		let (opts, args) = gnu_getopt(&cmdline, "ab:", &["alpha", "beta="])?;
		assert_eq!(args, ["arg1", "-", "-", "arg2"]);
		assert_eq!(
			opts,
			pairs(&[
				("-a", ""),
				("-b", "1"),
				("--alpha", ""),
				("--beta", "2"),
				("--beta", "3")
			])
		);

		let (opts, args) = gnu_getopt(
			&[
				"-a", "-", "-b", "-", "--alpha", "--beta=2", "--beta", "3", "arg1",
			],
			"ab:",
			&["alpha", "beta="],
		)?;
		assert_eq!(args, ["-", "arg1"]);
		assert_eq!(opts[1], ("-b".to_owned(), "-".to_owned()));

		// Posix style via +.
		let (opts, args) = gnu_getopt(&cmdline, "+ab:", &["alpha", "beta="])?;
		assert_eq!(opts, pairs(&[("-a", "")]));
		assert_eq!(args.len(), 11);
		Ok(())
	}

	#[test]
	fn errors() {
		let err = |args: &[&str]| {
			gnu_getopt(args, "ab:", &["alpha", "alpine", "beta="])
				.err()
				.map(|e| e.0)
		};
		assert_eq!(err(&["-c"]).as_deref(), Some("option -c not recognized"));
		assert_eq!(err(&["-b"]).as_deref(), Some("option -b requires argument"));
		assert_eq!(
			err(&["--al"]).as_deref(),
			Some("option --al not a unique prefix")
		);
		assert_eq!(
			err(&["--gamma"]).as_deref(),
			Some("option --gamma not recognized")
		);
		assert_eq!(
			err(&["--alpha=1"]).as_deref(),
			Some("option --alpha must not have an argument")
		);
		assert_eq!(
			err(&["--beta"]).as_deref(),
			Some("option --beta requires argument")
		);
		assert_eq!(
			gnu_getopt(&["--alph", "-bx"], "ab:", &["alpha", "beta="]).map(|r| r.0),
			Ok(pairs(&[("--alpha", ""), ("-b", "x")]))
		);
	}
}
