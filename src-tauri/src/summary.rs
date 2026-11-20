//! Turns raw activity spans into the numbers the dashboard shows.

use std::collections::HashMap;

use serde::Serialize;

use crate::categories::{self, Productivity};
use crate::db::{ActivityRow, ManualSession};
use crate::settings::Settings;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTotal {
    pub app: String,
    pub category: String,
    pub productivity: Productivity,
    pub secs: i64,
    /// Top window titles (only when title capture is on).
    pub top_titles: Vec<TitleTotal>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleTotal {
    pub title: String,
    pub secs: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotal {
    pub category: String,
    pub productivity: Productivity,
    pub secs: i64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSpan {
    pub start: i64,
    pub end: i64,
    pub app: String,
    pub category: String,
    pub productivity: Productivity,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FocusBlock {
    pub start: i64,
    pub end: i64,
    /// Time inside the block actually spent in productive apps.
    pub productive_secs: i64,
    pub top_app: String,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub from: i64,
    pub to: i64,
    pub total_secs: i64,
    pub productive_secs: i64,
    pub neutral_secs: i64,
    pub distracting_secs: i64,
    pub focus_secs: i64,
    pub longest_focus_secs: i64,
    pub first_activity: Option<i64>,
    pub last_activity: Option<i64>,
    pub context_switches: usize,
    pub apps: Vec<AppTotal>,
    pub categories: Vec<CategoryTotal>,
    pub timeline: Vec<TimelineSpan>,
    pub focus_blocks: Vec<FocusBlock>,
    pub manual_sessions: Vec<ManualSession>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntervalTotal {
    pub from: i64,
    pub to: i64,
    pub total_secs: i64,
    pub productive_secs: i64,
    pub focus_secs: i64,
}

pub struct Categorizer<'a> {
    overrides: &'a HashMap<String, String>,
}

impl<'a> Categorizer<'a> {
    pub fn new(overrides: &'a HashMap<String, String>) -> Self {
        Self { overrides }
    }

    pub fn category(&self, app: &str) -> String {
        match self.overrides.get(app) {
            Some(c) if categories::is_valid(c) => c.clone(),
            _ => categories::guess(app).to_string(),
        }
    }
}

/// Spans clamped to `[from, to)`, with zero-length pieces removed.
fn clamp(rows: &[ActivityRow], from: i64, to: i64) -> Vec<ActivityRow> {
    rows.iter()
        .filter_map(|r| {
            let start = r.start.max(from);
            let end = r.end.min(to);
            (end > start).then(|| ActivityRow {
                start,
                end,
                app: r.app.clone(),
                title: r.title.clone(),
            })
        })
        .collect()
}

pub fn compute(
    rows: &[ActivityRow],
    manual: Vec<ManualSession>,
    cats: &Categorizer<'_>,
    settings: &Settings,
    from: i64,
    to: i64,
) -> Summary {
    let rows = clamp(rows, from, to);
    let mut s = Summary {
        from,
        to,
        manual_sessions: manual,
        ..Default::default()
    };
    if rows.is_empty() {
        return s;
    }

    let mut cat_cache: HashMap<String, (String, Productivity)> = HashMap::new();
    let mut resolve = |app: &str| -> (String, Productivity) {
        cat_cache
            .entry(app.to_string())
            .or_insert_with(|| {
                let c = cats.category(app);
                let p = categories::productivity_of(&c);
                (c, p)
            })
            .clone()
    };

    let mut app_secs: HashMap<&str, i64> = HashMap::new();
    let mut title_secs: HashMap<(&str, &str), i64> = HashMap::new();
    let mut cat_secs: HashMap<String, (Productivity, i64)> = HashMap::new();
    let mut timeline: Vec<TimelineSpan> = Vec::new();
    let mut prev_app: Option<(&str, i64)> = None;

    for r in &rows {
        let d = r.end - r.start;
        let (cat, prod) = resolve(&r.app);
        s.total_secs += d;
        match prod {
            Productivity::Productive => s.productive_secs += d,
            Productivity::Neutral => s.neutral_secs += d,
            Productivity::Distracting => s.distracting_secs += d,
        }
        *app_secs.entry(&r.app).or_default() += d;
        if let Some(t) = r.title.as_deref().filter(|t| !t.is_empty()) {
            *title_secs.entry((&r.app, t)).or_default() += d;
        }
        cat_secs.entry(cat.clone()).or_insert((prod, 0)).1 += d;

        // Context switch = moving to a different app within a minute.
        if let Some((pa, pend)) = prev_app {
            if pa != r.app && r.start - pend <= 60 {
                s.context_switches += 1;
            }
        }
        prev_app = Some((&r.app, r.end));

        // Merge adjacent spans of the same app (different titles) for the timeline.
        match timeline.last_mut() {
            Some(last) if last.app == r.app && r.start - last.end <= 10 => {
                last.end = last.end.max(r.end)
            }
            _ => timeline.push(TimelineSpan {
                start: r.start,
                end: r.end,
                app: r.app.clone(),
                category: cat,
                productivity: prod,
            }),
        }
    }

    s.first_activity = rows.iter().map(|r| r.start).min();
    s.last_activity = rows.iter().map(|r| r.end).max();

    let mut titles_by_app: HashMap<&str, Vec<TitleTotal>> = HashMap::new();
    for ((app, title), secs) in title_secs {
        titles_by_app.entry(app).or_default().push(TitleTotal {
            title: title.to_string(),
            secs,
        });
    }
    let mut apps: Vec<AppTotal> = app_secs
        .into_iter()
        .map(|(app, secs)| {
            let (category, productivity) = resolve(app);
            let mut top_titles = titles_by_app.remove(app).unwrap_or_default();
            top_titles.sort_by(|a, b| b.secs.cmp(&a.secs).then_with(|| a.title.cmp(&b.title)));
            top_titles.truncate(5);
            AppTotal {
                app: app.to_string(),
                category,
                productivity,
                secs,
                top_titles,
            }
        })
        .collect();
    apps.sort_by(|a, b| b.secs.cmp(&a.secs).then_with(|| a.app.cmp(&b.app)));
    s.apps = apps;

    let mut categories: Vec<CategoryTotal> = cat_secs
        .into_iter()
        .map(|(category, (productivity, secs))| CategoryTotal {
            category,
            productivity,
            secs,
        })
        .collect();
    categories.sort_by(|a, b| {
        b.secs
            .cmp(&a.secs)
            .then_with(|| a.category.cmp(&b.category))
    });
    s.categories = categories;

    s.focus_blocks = detect_focus(&timeline, settings);
    s.focus_secs = s.focus_blocks.iter().map(|b| b.end - b.start).sum();
    s.longest_focus_secs = s
        .focus_blocks
        .iter()
        .map(|b| b.end - b.start)
        .max()
        .unwrap_or(0);
    s.timeline = timeline;
    s
}

/// A focus session is a stretch of productive work where no interruption
/// (idle, or non-productive apps) lasts longer than the break tolerance,
/// and that lasts at least the minimum focus length.
pub fn detect_focus(timeline: &[TimelineSpan], settings: &Settings) -> Vec<FocusBlock> {
    let tolerance = i64::from(settings.focus_break_tolerance_minutes) * 60;
    let min_len = i64::from(settings.focus_min_minutes) * 60;

    struct Cur<'a> {
        start: i64,
        end: i64,
        productive: i64,
        apps: HashMap<&'a str, i64>,
    }
    let finish = |c: Cur<'_>, out: &mut Vec<FocusBlock>| {
        if c.end - c.start >= min_len {
            let top_app = c
                .apps
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(a, _)| a.to_string())
                .unwrap_or_default();
            out.push(FocusBlock {
                start: c.start,
                end: c.end,
                productive_secs: c.productive,
                top_app,
            });
        }
    };

    let mut out = Vec::new();
    let mut cur: Option<Cur<'_>> = None;
    for span in timeline
        .iter()
        .filter(|t| t.productivity == Productivity::Productive)
    {
        let d = span.end - span.start;
        match cur.as_mut() {
            Some(c) if span.start - c.end <= tolerance => {
                c.end = c.end.max(span.end);
                c.productive += d;
                *c.apps.entry(&span.app).or_default() += d;
            }
            _ => {
                if let Some(c) = cur.take() {
                    finish(c, &mut out);
                }
                let mut apps = HashMap::new();
                apps.insert(span.app.as_str(), d);
                cur = Some(Cur {
                    start: span.start,
                    end: span.end,
                    productive: d,
                    apps,
                });
            }
        }
    }
    if let Some(c) = cur {
        finish(c, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(start: i64, end: i64, app: &str) -> ActivityRow {
        ActivityRow {
            start,
            end,
            app: app.into(),
            title: None,
        }
    }

    #[test]
    fn totals_and_focus() {
        let overrides = HashMap::new();
        let cats = Categorizer::new(&overrides);
        let settings = Settings::default(); // 25 min focus, 5 min tolerance
        let m = 60;
        let rows = vec![
            row(0, 20 * m, "Code"),
            row(20 * m, 23 * m, "Slack"), // short interruption: tolerated
            row(23 * m, 40 * m, "Code"),
            row(60 * m, 70 * m, "Code"), // 20 min gap: new block, too short
            row(70 * m, 80 * m, "Spotify"),
        ];
        let s = compute(&rows, vec![], &cats, &settings, 0, 24 * 3600);
        assert_eq!(s.total_secs, 60 * m);
        assert_eq!(s.productive_secs, 47 * m);
        assert_eq!(s.neutral_secs, 3 * m);
        assert_eq!(s.distracting_secs, 10 * m);
        assert_eq!(s.focus_blocks.len(), 1);
        assert_eq!(s.focus_blocks[0].start, 0);
        assert_eq!(s.focus_blocks[0].end, 40 * m);
        assert_eq!(s.focus_blocks[0].productive_secs, 37 * m);
        assert_eq!(s.focus_blocks[0].top_app, "Code");
        assert_eq!(s.longest_focus_secs, 40 * m);
        assert_eq!(s.apps[0].app, "Code");
        assert_eq!(s.apps[0].secs, 47 * m);
        assert_eq!(s.first_activity, Some(0));
        assert_eq!(s.last_activity, Some(80 * m));
        assert_eq!(s.context_switches, 3);
    }

    #[test]
    fn clamps_to_range_and_respects_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert("Spotify".to_string(), "Coding".to_string());
        overrides.insert("Weird".to_string(), "NotACategory".to_string());
        let cats = Categorizer::new(&overrides);
        let rows = vec![row(-100, 100, "Spotify"), row(100, 200, "Weird")];
        let s = compute(&rows, vec![], &cats, &Settings::default(), 0, 150);
        assert_eq!(s.total_secs, 150);
        assert_eq!(s.productive_secs, 100);
        assert_eq!(
            s.apps.iter().find(|a| a.app == "Weird").unwrap().category,
            "Other"
        );
    }

    #[test]
    fn empty() {
        let o = HashMap::new();
        let s = compute(
            &[],
            vec![],
            &Categorizer::new(&o),
            &Settings::default(),
            0,
            10,
        );
        assert_eq!(s.total_secs, 0);
        assert!(s.focus_blocks.is_empty());
    }
}
