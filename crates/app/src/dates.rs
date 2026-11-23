//! Dates as the Mac writes them in the UI's language and the Mac's region (TECH-NOTES §12): no
//! month or weekday names in the catalogs, so a new language needs only its `l10n::Lang`.
//! Always the Gregorian calendar: a journal's file names are.

use l10n::Lang;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Form {
    /// Monday, 23 November 2026: a journal day's heading.
    Long,
    /// 23 Nov 2026: the short heading.
    Short,
    /// November 2026: the newest month of a year in a journal.
    MonthYear,
    /// November: the other months.
    Month,
    /// Mon 23: a day in a journal.
    WeekdayDay,
    /// 23 Nov: when a file or a comment changed.
    MonthDay,
}

impl Form {
    const ALL: [Form; 6] = [Form::Long, Form::Short, Form::MonthYear, Form::Month, Form::WeekdayDay, Form::MonthDay];

    /// A Unicode date template; the locale picks the order and the punctuation.
    fn template(self) -> &'static str {
        match self {
            Form::Long => "EEEEdMMMMy",
            Form::Short => "dMMMy",
            Form::MonthYear => "LLLLy",
            Form::Month => "LLLL",
            Form::WeekdayDay => "EEEd",
            Form::MonthDay => "MMMd",
        }
    }
}

/// The date in the UI's language; `None` if there's no such day.
pub fn format(year: i32, month: u32, day: u32, form: Form) -> Option<String> {
    format_in(l10n::current(), year, month, day, form)
}

pub fn format_in(lang: Lang, year: i32, month: u32, day: u32, form: Form) -> Option<String> {
    Some(platform::format(lang, form.template(), true, noon(year, month, day)?))
}

fn noon(year: i32, month: u32, day: u32) -> Option<i64> {
    Some(chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(12, 0, 0)?.and_utc().timestamp())
}

/// Month names as they stand in a date (Russian's "ноября", not "ноябрь"), long and short.
const MONTH_PATTERNS: [&str; 4] = ["MMMM", "MMM", "LLLL", "LLL"];

/// Whether `text` is this date and nothing else, as any language the app has writes it in any
/// [`Form`] with a year, or as ISO: words in any order, case and punctuation aside. So a day
/// written under another UI language, an older macOS or Dostoevsky 1.0 (whose catalogs spelt
/// the dates) still reads as its date.
pub fn says(text: &str, year: i32, month: u32, day: u32) -> bool {
    // What the app writes now, without asking the Mac about six other languages.
    let lower = text.trim().to_lowercase();
    if [Form::Long, Form::Short].into_iter().any(|f| format(year, month, day, f).is_some_and(|s| s.to_lowercase() == lower)) {
        return true;
    }
    let tokens = words(text);
    let (numbers, names): (Vec<&String>, Vec<&String>) = tokens.iter().partition(|t| t.chars().all(|c| c.is_ascii_digit()));
    let numbers: Vec<i64> = numbers.iter().filter_map(|n| n.parse().ok()).collect();
    let mut sorted = numbers.clone();
    sorted.sort_unstable();
    let with = |extra: &[i64]| {
        let mut want: Vec<i64> = [i64::from(year), i64::from(day)].into_iter().chain(extra.iter().copied()).collect();
        want.sort_unstable();
        want == sorted
    };
    let iso = with(&[i64::from(month)]) && names.is_empty();
    if iso {
        return true;
    }
    if !with(&[]) || names.is_empty() {
        return false;
    }
    let Some(at) = noon(year, month, day) else { return false };
    let mut known = std::collections::HashSet::new();
    let mut months = std::collections::HashSet::new();
    for lang in Lang::ALL {
        for form in Form::ALL {
            known.extend(words(&platform::format(lang, form.template(), true, at)));
        }
        for pattern in MONTH_PATTERNS {
            months.extend(words(&platform::format(lang, pattern, false, at)));
        }
    }
    // An abbreviation is the name's start, however a language or a macOS cuts it ("мар." for "марта").
    let month = |n: &str| n.chars().count() >= 3 && months.iter().any(|m| m.starts_with(n));
    names.iter().all(|n| known.contains(*n) || month(n)) && names.iter().any(|n| month(n))
}

/// Lowercase runs of letters and digits.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

#[cfg(target_os = "macos")]
mod platform {
    use l10n::Lang;
    use objc2::rc::Retained;
    use objc2::AllocAnyThread as _;
    use objc2_foundation::{NSCalendar, NSCalendarIdentifierGregorian, NSDate, NSDateFormatter, NSLocale, NSString, NSTimeZone};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// (language, pattern, whether it's a template).
    type Key = (Lang, &'static str, bool);

    thread_local! {
        /// A formatter costs more to make than to use; the journal list asks for thousands.
        static FORMATTERS: RefCell<HashMap<Key, Retained<NSDateFormatter>>> = RefCell::new(HashMap::new());
    }

    /// The UI's language with the Mac's region: English on a Mac set to Russia writes 23 November.
    /// Tests use Britain's, whatever the Mac says.
    fn locale_id(lang: Lang) -> String {
        let code = lang.code().replace('-', "_");
        if code.contains('_') {
            return code;
        }
        let region = if cfg!(test) { Some("GB".to_string()) } else { NSLocale::currentLocale().regionCode().map(|r| r.to_string()) };
        match region {
            Some(r) => format!("{code}_{r}"),
            None => code,
        }
    }

    /// `template` lets the locale order it ([`Form::template`]); otherwise it is the exact pattern.
    pub fn format(lang: Lang, pattern: &'static str, template: bool, unix: i64) -> String {
        FORMATTERS.with(|f| {
            let mut f = f.borrow_mut();
            let formatter = f.entry((lang, pattern, template)).or_insert_with(|| {
                let formatter = NSDateFormatter::new();
                let locale = NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &NSString::from_str(&locale_id(lang)));
                formatter.setLocale(Some(&locale));
                // SAFETY: a constant Foundation exports.
                let gregorian = NSCalendar::calendarWithIdentifier(unsafe { NSCalendarIdentifierGregorian });
                formatter.setCalendar(gregorian.as_deref());
                formatter.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
                if template {
                    formatter.setLocalizedDateFormatFromTemplate(&NSString::from_str(pattern));
                } else {
                    formatter.setDateFormat(Some(&NSString::from_str(pattern)));
                }
                formatter
            });
            formatter.stringFromDate(&NSDate::dateWithTimeIntervalSince1970(unix as f64)).to_string()
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use l10n::Lang;

    /// English only, as Britain writes it.
    pub fn format(_: Lang, pattern: &'static str, _: bool, unix: i64) -> String {
        let Some(d) = chrono::DateTime::from_timestamp(unix, 0) else { return String::new() };
        d.format(match pattern {
            "EEEEdMMMMy" => "%A, %-d %B %Y",
            "dMMMy" => "%-d %b %Y",
            "LLLLy" => "%B %Y",
            "LLLL" | "MMMM" => "%B",
            "EEEd" => "%a %-d",
            "MMMd" => "%-d %b",
            _ => "%b",
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{format_in, says, Form};
    use l10n::Lang;

    #[test]
    fn the_mac_writes_the_date_in_each_language() {
        assert_eq!(format_in(Lang::En, 2026, 11, 15, Form::Long).as_deref(), Some("Sunday, 15 November 2026"));
        assert_eq!(format_in(Lang::En, 2026, 11, 15, Form::Short).as_deref(), Some("15 Nov 2026"));
        assert_eq!(format_in(Lang::En, 2026, 11, 15, Form::Month).as_deref(), Some("November"));
        assert_eq!(format_in(Lang::En, 2026, 2, 30, Form::Long), None);
        let ru = format_in(Lang::Ru, 2026, 11, 23, Form::Long).unwrap();
        assert!(ru.starts_with("понедельник, 23 ноября 2026"), "{ru}");
        assert_eq!(format_in(Lang::Ru, 2026, 11, 23, Form::Month).as_deref(), Some("ноябрь"), "the month alone, not 'of November'");
    }

    #[test]
    fn a_date_reads_as_itself_in_any_language_and_form() {
        for lang in Lang::ALL {
            for form in [Form::Long, Form::Short, Form::MonthYear] {
                let s = format_in(lang, 2026, 11, 23, form).unwrap();
                assert_eq!(says(&s, 2026, 11, 23), form != Form::MonthYear, "{lang:?} {form:?}: {s}");
            }
        }
        // What Dostoevsky 1.0 wrote from its catalogs.
        for old in ["Monday, 23 November 2026", "понедельник, 23 ноября 2026", "23 Nov 2026", "Montag, 23 November 2026", "lunes, 23 de noviembre de 2026", "2026-11-23"] {
            assert!(says(old, 2026, 11, 23), "{old}");
        }
        for old in ["5 мар. 2026", "5 Mär 2026"] {
            assert!(says(old, 2026, 3, 5), "{old}");
        }
        for not in ["Monday, 23 November 2025", "Tuesday 2026", "23 2026", "Trip to the river", "23 November", "Sunday, 22 November 2026 notes", "2026-11-22"] {
            assert!(!says(not, 2026, 11, 23), "{not}");
        }
    }
}
