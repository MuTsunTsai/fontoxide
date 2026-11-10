//! Port of `fontTools.misc.timeTools`: OpenType timestamps (seconds since
//! 1904-01-01 00:00:00 UTC).

/// Seconds from 1970-01-01 to 1904-01-01 (`epoch_diff`); negative.
pub const EPOCH_DIFF: i64 = -2_082_844_800;

const DAYNAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHNAMES: [&str; 12] = [
	"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A broken-down UTC time, like Python's `time.struct_time` from `gmtime()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tm {
	/// Full year, e.g. 1970.
	pub year: i64,
	/// 1–12.
	pub month: u32,
	/// 1–31.
	pub day: u32,
	/// 0–23.
	pub hour: u32,
	/// 0–59.
	pub minute: u32,
	/// 0–59.
	pub second: u32,
	/// 0 = Monday … 6 = Sunday.
	pub weekday: u32,
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
	let z = z + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z.rem_euclid(146_097);
	let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
	let y = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
	let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
	(if m <= 2 { y + 1 } else { y }, m, d)
}

/// A civil date to days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
	let y = if m <= 2 { y - 1 } else { y };
	let era = y.div_euclid(400);
	let yoe = y.rem_euclid(400);
	let m = m as i64;
	let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	era * 146_097 + doe - 719_468
}

/// `time.gmtime(seconds)`.
pub fn gmtime(seconds: i64) -> Tm {
	let days = seconds.div_euclid(86_400);
	let secs = seconds.rem_euclid(86_400) as u32;
	let (year, month, day) = civil_from_days(days);
	Tm {
		year,
		month,
		day,
		hour: secs / 3600,
		minute: secs / 60 % 60,
		second: secs % 60,
		// 1970-01-01 was a Thursday.
		weekday: (days + 3).rem_euclid(7) as u32,
	}
}

/// Formats a time as `"Thu Jan  1 00:00:00 1970"` (`asctime`).
pub fn asctime(t: &Tm) -> String {
	format!(
		"{} {} {:>2} {:02}:{:02}:{:02} {}",
		DAYNAMES[t.weekday as usize],
		MONTHNAMES[(t.month - 1) as usize],
		t.day,
		t.hour,
		t.minute,
		t.second,
		// strftime("%Y") pads to at least four digits.
		if t.year >= 0 {
			format!("{:04}", t.year)
		} else {
			t.year.to_string()
		}
	)
}

/// Formats an OpenType timestamp (`timestampToString`). Timestamps before the Unix
/// epoch are clamped to it, as fontTools does.
pub fn timestamp_to_string(value: i64) -> String {
	asctime(&gmtime(value.saturating_add(EPOCH_DIFF).max(0)))
}

/// Parses a timestamp written by [`timestamp_to_string`] (`timestampFromString`).
/// Returns `None` if the string is malformed or the weekday is inconsistent.
pub fn timestamp_from_string(value: &str) -> Option<i64> {
	let head = value.get(..7)?;
	let mut parts = head.split_whitespace();
	let wkday = parts.next()?;
	let month = parts.next()?;
	if parts.next().is_some() {
		return None;
	}
	// The rest is " %d %H:%M:%S %Y"; strptime allows flexible whitespace.
	let mut rest = value[7..].split_whitespace();
	let day: u32 = rest.next()?.parse().ok()?;
	let mut hms = rest.next()?.split(':');
	let hour: u32 = hms.next()?.parse().ok()?;
	let minute: u32 = hms.next()?.parse().ok()?;
	let second: u32 = hms.next()?.parse().ok()?;
	let year: i64 = rest.next()?.parse().ok()?;
	if hms.next().is_some() || rest.next().is_some() {
		return None;
	}
	let month = MONTHNAMES.iter().position(|m| *m == month)? as u32 + 1;
	if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 61 {
		return None;
	}
	let days = days_from_civil(year, month, day);
	if civil_from_days(days) != (year, month, day) {
		return None;
	}
	let weekday = (days + 3).rem_euclid(7) as usize;
	if DAYNAMES.iter().position(|d| *d == wkday)? != weekday {
		return None;
	}
	let seconds = days * 86_400 + (hour * 3600 + minute * 60 + second) as i64;
	Some(seconds - EPOCH_DIFF)
}

/// The clock set by [`set_clock`].
static CLOCK: std::sync::RwLock<Option<fn() -> f64>> = std::sync::RwLock::new(None);

/// Sets the clock of [`timestamp_now`], a function returning the seconds since the
/// Unix epoch, in place of the system clock.
///
/// `std::time::SystemTime` is unavailable on `wasm32-unknown-unknown`, so set one
/// there (for example from `js_sys::Date::now() / 1000.0`) before writing fonts
/// whose `recalc_timestamp` is on.
pub fn set_clock(clock: fn() -> f64) {
	*CLOCK.write().unwrap_or_else(|e| e.into_inner()) = Some(clock);
}

/// The current time as an OpenType timestamp (`timestampNow`). Honors
/// `SOURCE_DATE_EPOCH` for reproducible builds, then the clock of [`set_clock`];
/// returns `None` if `SOURCE_DATE_EPOCH` is set but not an integer (fontTools
/// raises `ValueError`), or if there is no clock (on `wasm32-unknown-unknown`
/// without [`set_clock`]).
pub fn timestamp_now() -> Option<i64> {
	if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
		return Some(crate::py::parse_int(&epoch)? as i64 - EPOCH_DIFF);
	}
	let clock = *CLOCK.read().unwrap_or_else(|e| e.into_inner());
	let now = match clock {
		Some(clock) => clock(),
		None => system_time()?,
	};
	Some((now - EPOCH_DIFF as f64) as i64)
}

#[cfg(not(target_family = "wasm"))]
fn system_time() -> Option<f64> {
	Some(
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map(|d| d.as_secs_f64())
			.unwrap_or(0.0),
	)
}

#[cfg(target_family = "wasm")]
fn system_time() -> Option<f64> {
	None
}

/// Converts a Unix timestamp to an OpenType timestamp (`timestampSinceEpoch`).
pub fn timestamp_since_epoch(value: i64) -> i64 {
	value - EPOCH_DIFF
}

#[cfg(test)]
mod tests {
	//! Port of Tests/misc/timeTools_test.py.

	use super::*;

	#[test]
	fn test_asctime() {
		assert_eq!(asctime(&gmtime(0)), "Thu Jan  1 00:00:00 1970");
	}

	#[test]
	fn roundtrip() {
		for ts in [2_082_844_800_i64, 3_000_000_000, 3_786_576_000, 0xFFFF_FFFF] {
			let s = timestamp_to_string(ts);
			assert_eq!(timestamp_from_string(&s), Some(ts), "{s}");
		}
		assert_eq!(timestamp_to_string(0), "Thu Jan  1 00:00:00 1970");
		assert_eq!(
			timestamp_to_string(3_786_576_000),
			"Thu Dec 28 02:40:00 2023"
		);
		assert_eq!(timestamp_from_string("Fri Jan  1 00:00:00 1970"), None);
		assert_eq!(gmtime(951_782_400).day, 29);
	}

	// test_source_date_epoch mutates the process environment, which is not
	// thread-safe in Rust's test harness; the logic is covered by `timestamp_now`
	// being a direct port.

	/// No other test of this crate reads the clock, so setting it is safe.
	#[test]
	fn set_clock_overrides_system_time() {
		if std::env::var_os("SOURCE_DATE_EPOCH").is_some() {
			return;
		}
		set_clock(|| 1_000_000_000.5);
		assert_eq!(timestamp_now(), Some(1_000_000_000 - EPOCH_DIFF));
	}
}
