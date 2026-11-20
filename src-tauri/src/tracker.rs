//! Background sampler: every few seconds, records which app is in the
//! foreground, unless the user is idle, tracking is paused, or the app is excluded.
//!
//! Only the app name (and, if the user opts in, the window title) is read.
//! No keystrokes, screenshots, clipboard, or URLs are ever collected.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::settings::{clean_text, Settings, MAX_APP_NAME_LEN};

pub const POLL_SECS: i64 = 5;
/// If two ticks are further apart than this, the machine slept; don't bridge the gap.
const MAX_TICK_GAP: i64 = POLL_SECS * 4;
/// Write the open span to disk at least this often.
const FLUSH_EVERY: i64 = 10;
const MAX_TITLE_LEN: usize = 300;

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
struct Span {
    id: Option<i64>,
    app: String,
    title: Option<String>,
    start: i64,
    end: i64,
    flushed_end: i64,
}

#[derive(Default)]
struct State {
    current: Option<Span>,
    last_tick: Option<i64>,
}

pub struct Shared {
    pub paused: AtomicBool,
    pub settings: RwLock<Settings>,
    state: Mutex<State>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Current {
    pub app: String,
    pub since: i64,
}

impl Shared {
    pub fn new(settings: Settings) -> Self {
        Self {
            paused: AtomicBool::new(false),
            settings: RwLock::new(settings),
            state: Mutex::default(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn current(&self) -> Option<Current> {
        let st = crate::lock(&self.state);
        st.current.as_ref().map(|c| Current {
            app: c.app.clone(),
            since: c.start,
        })
    }

    /// Persist the open span so queries include up-to-the-second data.
    pub fn flush(&self, db: &Mutex<Connection>) {
        {
            let mut st = crate::lock(&self.state);
            if let Some(span) = st.current.as_mut() {
                write_span(db, span);
            }
        }
    }

    /// Close the open span (used on pause, sleep, and shutdown).
    pub fn close(&self, db: &Mutex<Connection>) {
        {
            let mut st = crate::lock(&self.state);
            close_at(db, &mut st, None);
        }
    }

    pub fn set_paused(&self, db: &Mutex<Connection>, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
        if paused {
            self.close(db);
        }
    }

    /// Forget any unsaved title on the open span (used when title capture is turned off).
    pub fn drop_titles(&self, db: &Mutex<Connection>) {
        {
            let mut st = crate::lock(&self.state);
            if st.current.as_ref().is_some_and(|c| c.title.is_some()) {
                close_at(db, &mut st, None);
            }
        }
    }

    fn tick(&self, db: &Mutex<Connection>) {
        let now = unix_now();
        let mut st = crate::lock(&self.state);
        let prev = st.last_tick.replace(now);
        let continuous = prev.is_some_and(|p| now >= p && now - p <= MAX_TICK_GAP);
        if !continuous {
            close_at(db, &mut st, None);
        }
        if self.paused.load(Ordering::SeqCst) {
            close_at(db, &mut st, None);
            return;
        }

        let settings = self.settings();
        let idle = idle_secs();
        if idle >= i64::from(settings.idle_threshold_secs) {
            // Trim the span back to when input stopped.
            close_at(db, &mut st, Some(now - idle));
            return;
        }

        let Some((app, raw_title)) = active_window() else {
            close_at(db, &mut st, None);
            return;
        };
        if settings.is_excluded(&app) {
            close_at(db, &mut st, None);
            return;
        }
        let title = settings
            .capture_titles
            .then(|| clean_text(&raw_title, MAX_TITLE_LEN))
            .filter(|t| !t.is_empty());

        match st.current.as_mut() {
            Some(c) if c.app == app && c.title == title => {
                c.end = now;
                if now - c.flushed_end >= FLUSH_EVERY {
                    write_span(db, c);
                }
            }
            _ => {
                if let Some(c) = st.current.as_mut() {
                    c.end = now;
                }
                close_at(db, &mut st, None);
                st.current = Some(Span {
                    id: None,
                    app,
                    title,
                    start: now,
                    end: now,
                    flushed_end: now,
                });
            }
        }
    }
}

fn close_at(db: &Mutex<Connection>, st: &mut State, at: Option<i64>) {
    if let Some(mut span) = st.current.take() {
        if let Some(t) = at {
            span.end = span.end.min(t).max(span.start);
        }
        write_span(db, &mut span);
    }
}

fn write_span(db: &Mutex<Connection>, span: &mut Span) {
    let conn = crate::lock(db);
    let result = match span.id {
        None if span.end > span.start => db::insert_activity(
            &conn,
            span.start,
            span.end,
            &span.app,
            span.title.as_deref(),
        )
        .map(|id| {
            span.id = Some(id);
        }),
        None => Ok(()),
        Some(id) if span.end > span.start => db::update_activity_end(&conn, id, span.end),
        Some(id) => db::delete_activity(&conn, id).map(|_| span.id = None),
    };
    match result {
        Ok(()) => span.flushed_end = span.end,
        Err(e) => eprintln!("stint: failed to save activity: {e}"),
    }
}

fn idle_secs() -> i64 {
    user_idle::UserIdle::get_time()
        .map(|u| u.as_seconds() as i64)
        .unwrap_or(0)
}

fn active_window() -> Option<(String, String)> {
    let w = active_win_pos_rs::get_active_window().ok()?;
    let mut app = clean_text(&w.app_name, MAX_APP_NAME_LEN);
    if app.is_empty() {
        app = w
            .process_path
            .file_stem()
            .map(|s| clean_text(&s.to_string_lossy(), MAX_APP_NAME_LEN))
            .unwrap_or_default();
    }
    (!app.is_empty()).then_some((app, w.title))
}

pub fn spawn(db: Arc<Mutex<Connection>>, shared: Arc<Shared>) {
    thread::Builder::new()
        .name("stint-tracker".into())
        .spawn(move || loop {
            // A panic inside one tick must never take the tracker down.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.tick(&db)));
            thread::sleep(Duration::from_secs(POLL_SECS as u64));
        })
        .expect("failed to start tracker thread");
}
