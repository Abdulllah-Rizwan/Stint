//! Export of the user's own data to CSV or JSON.

use std::collections::HashMap;

use serde::Serialize;

use crate::db::ActivityRow;
use crate::summary::Categorizer;

#[derive(Serialize)]
struct JsonRow<'a> {
    start: String,
    end: String,
    duration_secs: i64,
    app: &'a str,
    category: String,
    title: Option<&'a str>,
}

pub fn to_json(rows: &[ActivityRow], overrides: &HashMap<String, String>) -> String {
    let cats = Categorizer::new(overrides);
    let out: Vec<JsonRow<'_>> = rows
        .iter()
        .map(|r| JsonRow {
            start: iso_utc(r.start),
            end: iso_utc(r.end),
            duration_secs: r.end - r.start,
            app: &r.app,
            category: cats.category(&r.app),
            title: r.title.as_deref(),
        })
        .collect();
    serde_json::to_string_pretty(&out).unwrap_or_else(|_| "[]".into())
}

pub fn to_csv(rows: &[ActivityRow], overrides: &HashMap<String, String>) -> String {
    let cats = Categorizer::new(overrides);
    let mut out = String::from("start_utc,end_utc,duration_secs,app,category,title\n");
    for r in rows {
        let fields = [
            iso_utc(r.start),
            iso_utc(r.end),
            (r.end - r.start).to_string(),
            csv_field(&r.app),
            csv_field(&cats.category(&r.app)),
            csv_field(r.title.as_deref().unwrap_or("")),
        ];
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

/// Quote a CSV field and neutralise spreadsheet formula injection
/// (window titles are attacker-controllable: any web page sets its own title).
fn csv_field(s: &str) -> String {
    let mut v = s.replace(['\r', '\n'], " ");
    if v.starts_with(['=', '+', '-', '@', '\t']) {
        v.insert(0, '\'');
    }
    format!("\"{}\"", v.replace('"', "\"\""))
}

/// Unix seconds -> `YYYY-MM-DDTHH:MM:SSZ` (proleptic Gregorian, UTC).
pub fn iso_utc(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("line1\nline2"), "\"line1 line2\"");
    }
}
