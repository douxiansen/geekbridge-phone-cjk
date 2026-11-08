// SPDX-License-Identifier: GPL-3.0-or-later
//! A small SQLite cache of what was fetched from the phone: contacts, texts
//! and call history. It makes startup instant and lets the service write only
//! what changed instead of rewriting whole files.
//!
//! Everything is keyed by the phone's address, so several phones can share one
//! database. Settings stay in a JSON file: they are tiny and hand-editable.

use crate::contacts::Contact;
use crate::conversations::Message;
use crate::recents::{Kind, Recent};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// The most texts kept per phone; the oldest go first.
pub const MAX_MESSAGES: i64 = 5000;
const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

pub struct Db {
    conn: Mutex<Connection>,
}

fn kind_to_i(k: Kind) -> i64 {
    match k {
        Kind::Incoming => 0,
        Kind::Outgoing => 1,
        Kind::Missed => 2,
    }
}

fn kind_from_i(i: i64) -> Kind {
    match i {
        1 => Kind::Outgoing,
        2 => Kind::Missed,
        _ => Kind::Incoming,
    }
}

impl Db {
    /// Opens (creating if needed) the database at `path`, owner-only on Unix.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            create_private_dir(dir)?;
        }
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // WAL keeps readers and the single writer out of each other's way.
        let _: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
            .unwrap_or_default();
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.lock();
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE contacts (
                     phone TEXT NOT NULL, position INTEGER NOT NULL,
                     name TEXT NOT NULL, numbers TEXT NOT NULL,
                     PRIMARY KEY (phone, position));
                 CREATE TABLE messages (
                     phone TEXT NOT NULL, ident TEXT NOT NULL,
                     number TEXT NOT NULL, incoming INTEGER NOT NULL,
                     body TEXT NOT NULL, at INTEGER NOT NULL, read INTEGER NOT NULL,
                     PRIMARY KEY (phone, ident));
                 CREATE INDEX messages_by_time ON messages (phone, at);
                 CREATE TABLE recents (
                     phone TEXT NOT NULL, number TEXT NOT NULL, kind INTEGER NOT NULL,
                     at INTEGER NOT NULL, seconds INTEGER NOT NULL,
                     PRIMARY KEY (phone, number, kind, at));
                 CREATE TABLE meta (phone TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
                     PRIMARY KEY (phone, key));",
            )?;
        }
        if version != SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    // --- contacts -----------------------------------------------------------

    pub fn load_contacts(&self, phone: &str) -> Result<Vec<Contact>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT name, numbers FROM contacts WHERE phone = ?1 ORDER BY position")?;
        let rows = stmt.query_map([phone], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (name, numbers) = row?;
            out.push(Contact {
                name,
                numbers: serde_json::from_str(&numbers).unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// Replaces the phone's contacts and stamps the fetch time.
    pub fn replace_contacts(
        &self,
        phone: &str,
        contacts: &[Contact],
        fetched_at: i64,
    ) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM contacts WHERE phone = ?1", [phone])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO contacts (phone, position, name, numbers) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (i, c) in contacts.iter().enumerate() {
                ins.execute(params![
                    phone,
                    i as i64,
                    c.name,
                    serde_json::to_string(&c.numbers).unwrap_or_default()
                ])?;
            }
        }
        tx.execute("INSERT OR REPLACE INTO meta (phone, key, value) VALUES (?1, 'contacts_fetched_at', ?2)", params![phone, fetched_at.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// When the contacts were last fetched from the phone (unix seconds).
    pub fn contacts_fetched_at(&self, phone: &str) -> Result<Option<i64>> {
        let conn = self.lock();
        let v: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE phone = ?1 AND key = 'contacts_fetched_at'",
                [phone],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.and_then(|s| s.parse().ok()))
    }

    // --- messages -----------------------------------------------------------

    pub fn load_messages(&self, phone: &str) -> Result<Vec<Message>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT number, incoming, body, at, read FROM messages WHERE phone = ?1 ORDER BY at",
        )?;
        let rows = stmt.query_map([phone], |r| {
            Ok(Message {
                number: r.get(0)?,
                incoming: r.get::<_, i64>(1)? != 0,
                text: r.get(2)?,
                at: r.get(3)?,
                read: r.get::<_, i64>(4)? != 0,
                pending: false,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Inserts new texts and updates the read flag of known ones.
    pub fn upsert_messages(&self, phone: &str, messages: &[Message]) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        {
            let mut up = tx.prepare(
                "INSERT INTO messages (phone, ident, number, incoming, body, at, read) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (phone, ident) DO UPDATE SET read = MAX(read, excluded.read)",
            )?;
            for m in messages {
                up.execute(params![
                    phone,
                    m.identity(),
                    m.number,
                    i64::from(m.incoming),
                    m.text,
                    m.at,
                    i64::from(m.read)
                ])?;
            }
        }
        tx.execute(
            "DELETE FROM messages WHERE phone = ?1 AND ident NOT IN (SELECT ident FROM messages WHERE phone = ?1 ORDER BY at DESC LIMIT ?2)",
            params![phone, MAX_MESSAGES],
        )?;
        tx.commit()?;
        Ok(())
    }

    // --- recents ------------------------------------------------------------

    pub fn load_recents(&self, phone: &str) -> Result<Vec<Recent>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT number, kind, at, seconds FROM recents WHERE phone = ?1 ORDER BY at DESC",
        )?;
        let rows = stmt.query_map([phone], |r| {
            Ok(Recent {
                number: r.get(0)?,
                kind: kind_from_i(r.get(1)?),
                at: r.get(2)?,
                seconds: r.get::<_, i64>(3)?.max(0) as u64,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn upsert_recents(&self, phone: &str, recents: &[Recent]) -> Result<()> {
        if recents.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        {
            let mut ins = tx.prepare("INSERT OR IGNORE INTO recents (phone, number, kind, at, seconds) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for r in recents {
                ins.execute(params![
                    phone,
                    r.number,
                    kind_to_i(r.kind),
                    r.at,
                    r.seconds as i64
                ])?;
            }
        }
        tx.execute(
            "DELETE FROM recents WHERE phone = ?1 AND rowid NOT IN (SELECT rowid FROM recents WHERE phone = ?1 ORDER BY at DESC LIMIT 300)",
            [phone],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(n: &str, incoming: bool, t: &str, at: i64, read: bool) -> Message {
        Message {
            number: n.into(),
            incoming,
            text: t.into(),
            at,
            read,
            pending: false,
        }
    }

    #[test]
    fn contacts_round_trip_and_replace() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.load_contacts("P").unwrap().is_empty());
        let a = vec![
            Contact {
                name: "Alí".into(),
                numbers: vec!["+1".into(), "+2".into()],
            },
            Contact {
                name: "Zoé".into(),
                numbers: vec!["+3".into()],
            },
        ];
        db.replace_contacts("P", &a, 1000).unwrap();
        assert_eq!(db.load_contacts("P").unwrap(), a);
        assert_eq!(db.contacts_fetched_at("P").unwrap(), Some(1000));
        db.replace_contacts("P", &a[..1], 2000).unwrap();
        assert_eq!(db.load_contacts("P").unwrap().len(), 1);
        // Another phone is separate.
        assert!(db.load_contacts("Q").unwrap().is_empty());
        assert_eq!(db.contacts_fetched_at("Q").unwrap(), None);
    }

    #[test]
    fn messages_insert_once_and_read_only_turns_on() {
        let db = Db::open_in_memory().unwrap();
        let m = msg("+1", true, "hi", 10, false);
        db.upsert_messages("P", &[m.clone(), m.clone()]).unwrap();
        assert_eq!(db.load_messages("P").unwrap().len(), 1);
        let read = Message {
            read: true,
            ..m.clone()
        };
        db.upsert_messages("P", &[read]).unwrap();
        assert!(db.load_messages("P").unwrap()[0].read);
        db.upsert_messages("P", &[m]).unwrap(); // a stale "unread" must not undo it
        assert!(db.load_messages("P").unwrap()[0].read);
    }

    #[test]
    fn messages_load_oldest_first_and_are_capped() {
        let db = Db::open_in_memory().unwrap();
        let many: Vec<Message> = (0..(MAX_MESSAGES + 50))
            .map(|i| msg("+1", true, &format!("m{i}"), i, true))
            .collect();
        db.upsert_messages("P", &many).unwrap();
        let got = db.load_messages("P").unwrap();
        assert_eq!(got.len() as i64, MAX_MESSAGES);
        assert_eq!(got[0].at, 50); // the oldest 50 were dropped
        assert!(got.windows(2).all(|w| w[0].at <= w[1].at));
    }

    #[test]
    fn recents_round_trip_dedupe_and_order() {
        let db = Db::open_in_memory().unwrap();
        let r1 = Recent {
            number: "+1".into(),
            kind: Kind::Missed,
            at: 100,
            seconds: 0,
        };
        let r2 = Recent {
            number: "+2".into(),
            kind: Kind::Incoming,
            at: 200,
            seconds: 33,
        };
        db.upsert_recents("P", &[r1.clone(), r2.clone(), r1.clone()])
            .unwrap();
        assert_eq!(db.load_recents("P").unwrap(), vec![r2, r1]);
    }

    #[test]
    fn survives_reopen_and_is_private() {
        let dir = std::env::temp_dir().join(format!("gbp-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("cache.db");
        {
            let db = Db::open(&path).unwrap();
            db.upsert_messages("P", &[msg("+1", false, "kept", 5, true)])
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.load_messages("P").unwrap()[0].text, "kept");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
