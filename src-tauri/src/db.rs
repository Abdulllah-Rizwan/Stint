//! SQLite storage. Every query uses bound parameters; no SQL is built from user input.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::settings::Settings;

pub type DbResult<T> = rusqlite::Result<T>;

const SCHEMA_V1: &str = "
CREATE TABLE activity (
    id        INTEGER PRIMARY KEY,
    start_ts  INTEGER NOT NULL,
    end_ts    INTEGER NOT NULL,
    app       TEXT    NOT NULL,
    title     TEXT,
    CHECK (end_ts >= start_ts)
);
CREATE INDEX idx_activity_start ON activity(start_ts);
CREATE INDEX idx_activity_end   ON activity(end_ts);

CREATE TABLE focus_sessions (
    id             INTEGER PRIMARY KEY,
    start_ts       INTEGER NOT NULL,
    end_ts         INTEGER,
    target_minutes INTEGER NOT NULL,
    label          TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX idx_focus_start ON focus_sessions(start_ts);

CREATE TABLE app_categories (
    app      TEXT PRIMARY KEY,
    category TEXT NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

pub fn open(path: &Path) -> DbResult<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> DbResult<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> DbResult<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
    conn.execute_batch(
        "PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA secure_delete=ON;
         PRAGMA trusted_schema=OFF;",
    )
}

fn migrate(conn: &Connection) -> DbResult<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(&format!(
            "BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"
        ))?;
    }
    Ok(())
}

// ---------- activity ----------

#[derive(Debug, Clone, Serialize)]
pub struct ActivityRow {
    pub start: i64,
    pub end: i64,
    pub app: String,
    pub title: Option<String>,
}

pub fn insert_activity(
    conn: &Connection,
    start: i64,
    end: i64,
    app: &str,
    title: Option<&str>,
) -> DbResult<i64> {
    conn.execute(
        "INSERT INTO activity (start_ts, end_ts, app, title) VALUES (?1, ?2, ?3, ?4)",
        params![start, end, app, title],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_activity_end(conn: &Connection, id: i64, end: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE activity SET end_ts = MAX(start_ts, ?2) WHERE id = ?1",
        params![id, end],
    )?;
    Ok(())
}

pub fn delete_activity(conn: &Connection, id: i64) -> DbResult<()> {
    conn.execute("DELETE FROM activity WHERE id = ?1", params![id])?;
    Ok(())
}

/// Activity overlapping `[from, to)`, ordered by start time.
pub fn activity_in_range(conn: &Connection, from: i64, to: i64) -> DbResult<Vec<ActivityRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT start_ts, end_ts, app, title FROM activity
         WHERE end_ts > ?1 AND start_ts < ?2
         ORDER BY start_ts, id",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        Ok(ActivityRow {
            start: r.get(0)?,
            end: r.get(1)?,
            app: r.get(2)?,
            title: r.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn clear_titles(conn: &Connection) -> DbResult<()> {
    conn.execute(
        "UPDATE activity SET title = NULL WHERE title IS NOT NULL",
        [],
    )?;
    Ok(())
}

// ---------- categories ----------

pub fn category_overrides(conn: &Connection) -> DbResult<HashMap<String, String>> {
    let mut stmt = conn.prepare_cached("SELECT app, category FROM app_categories")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

pub fn set_category(conn: &Connection, app: &str, category: &str) -> DbResult<()> {
    conn.execute(
        "INSERT INTO app_categories (app, category) VALUES (?1, ?2)
         ON CONFLICT(app) DO UPDATE SET category = excluded.category",
        params![app, category],
    )?;
    Ok(())
}

// ---------- manual focus sessions ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualSession {
    pub id: i64,
    pub start: i64,
    pub end: Option<i64>,
    pub target_minutes: i64,
    pub label: String,
}

fn map_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<ManualSession> {
    Ok(ManualSession {
        id: r.get(0)?,
        start: r.get(1)?,
        end: r.get(2)?,
        target_minutes: r.get(3)?,
        label: r.get(4)?,
    })
}

pub fn active_focus(conn: &Connection) -> DbResult<Option<ManualSession>> {
    conn.query_row(
        "SELECT id, start_ts, end_ts, target_minutes, label FROM focus_sessions
         WHERE end_ts IS NULL ORDER BY start_ts DESC LIMIT 1",
        [],
        map_session,
    )
    .optional()
}

pub fn start_focus(
    conn: &Connection,
    now: i64,
    target_minutes: u32,
    label: &str,
) -> DbResult<ManualSession> {
    stop_focus(conn, now)?;
    conn.execute(
        "INSERT INTO focus_sessions (start_ts, target_minutes, label) VALUES (?1, ?2, ?3)",
        params![now, target_minutes, label],
    )?;
    Ok(ManualSession {
        id: conn.last_insert_rowid(),
        start: now,
        end: None,
        target_minutes: target_minutes.into(),
        label: label.to_string(),
    })
}

pub fn stop_focus(conn: &Connection, now: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE focus_sessions SET end_ts = MAX(start_ts, ?1) WHERE end_ts IS NULL",
        params![now],
    )?;
    Ok(())
}

pub fn focus_in_range(conn: &Connection, from: i64, to: i64) -> DbResult<Vec<ManualSession>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, start_ts, end_ts, target_minutes, label FROM focus_sessions
         WHERE COALESCE(end_ts, ?2) > ?1 AND start_ts < ?2
         ORDER BY start_ts",
    )?;
    let rows = stmt.query_map(params![from, to], map_session)?;
    rows.collect()
}

// ---------- settings ----------

pub fn load_settings(conn: &Connection) -> DbResult<Settings> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'settings'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .filter(|s| s.validate().is_ok())
        .unwrap_or_default())
}

pub fn save_settings(conn: &Connection, settings: &Settings) -> DbResult<()> {
    let json = serde_json::to_string(settings).expect("settings serialize");
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('settings', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![json],
    )?;
    Ok(())
}

// ---------- maintenance ----------

pub fn apply_retention(conn: &Connection, retention_days: u32, now: i64) -> DbResult<()> {
    if retention_days == 0 {
        return Ok(());
    }
    let cutoff = now - i64::from(retention_days) * 86_400;
    conn.execute("DELETE FROM activity WHERE end_ts < ?1", params![cutoff])?;
    conn.execute(
        "DELETE FROM focus_sessions WHERE end_ts IS NOT NULL AND end_ts < ?1",
        params![cutoff],
    )?;
    Ok(())
}

pub fn delete_all(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        "BEGIN;
         DELETE FROM activity;
         DELETE FROM focus_sessions;
         DELETE FROM app_categories;
         COMMIT;
         VACUUM;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_roundtrip() {
        let c = open_in_memory().unwrap();
        let id = insert_activity(&c, 100, 110, "Code", Some("main.rs")).unwrap();
        update_activity_end(&c, id, 150).unwrap();
        insert_activity(&c, 200, 260, "Slack", None).unwrap();
        let rows = activity_in_range(&c, 0, 1000).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].end, 150);
        assert_eq!(activity_in_range(&c, 155, 199).unwrap().len(), 0);
        clear_titles(&c).unwrap();
        assert!(activity_in_range(&c, 0, 1000).unwrap()[0].title.is_none());
    }

    #[test]
    fn hostile_strings_are_stored_verbatim() {
        let c = open_in_memory().unwrap();
        let evil = "x'); DROP TABLE activity; --";
        insert_activity(&c, 1, 2, evil, Some(evil)).unwrap();
        set_category(&c, evil, "Coding").unwrap();
        let rows = activity_in_range(&c, 0, 10).unwrap();
        assert_eq!(rows[0].app, evil);
        assert_eq!(category_overrides(&c).unwrap()[evil], "Coding");
    }

    #[test]
    fn focus_lifecycle() {
        let c = open_in_memory().unwrap();
        start_focus(&c, 1000, 25, "deep work").unwrap();
        assert!(active_focus(&c).unwrap().is_some());
        // Starting another closes the previous one.
        start_focus(&c, 2000, 50, "").unwrap();
        stop_focus(&c, 3000).unwrap();
        assert!(active_focus(&c).unwrap().is_none());
        let s = focus_in_range(&c, 0, 10_000).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].end, Some(2000));
        assert_eq!(s[1].end, Some(3000));
    }

    #[test]
    fn settings_roundtrip_and_retention() {
        let c = open_in_memory().unwrap();
        assert_eq!(load_settings(&c).unwrap(), Settings::default());
        let s = Settings {
            capture_titles: true,
            ..Default::default()
        };
        save_settings(&c, &s).unwrap();
        assert!(load_settings(&c).unwrap().capture_titles);

        insert_activity(&c, 0, 10, "Old", None).unwrap();
        insert_activity(&c, 9 * 86_400, 9 * 86_400 + 10, "New", None).unwrap();
        apply_retention(&c, 7, 10 * 86_400).unwrap();
        let rows = activity_in_range(&c, 0, i64::MAX).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].app, "New");
        delete_all(&c).unwrap();
        assert!(activity_in_range(&c, 0, i64::MAX).unwrap().is_empty());
    }
}
