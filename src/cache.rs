use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

pub struct Cache {
    connection: Connection,
    max_bytes: u64,
}

impl Cache {
    pub fn open(path: &Path, max_bytes: u64) -> Result<Self> {
        let connection =
            Connection::open(path).with_context(|| format!("无法打开缓存 {}", path.display()))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS translations (
                 cache_key BLOB PRIMARY KEY,
                 translated TEXT NOT NULL,
                 size_bytes INTEGER NOT NULL,
                 accessed_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS translations_lru
                 ON translations(accessed_at);",
        )?;
        Ok(Self {
            connection,
            max_bytes,
        })
    }

    pub fn key(parts: &[&str]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for value in parts {
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value.as_bytes());
        }
        hasher.finalize().into()
    }

    pub fn get(&mut self, key: &[u8]) -> Result<Option<String>> {
        let now = timestamp();
        let transaction = self.connection.transaction()?;
        let value = transaction
            .query_row(
                "SELECT translated FROM translations WHERE cache_key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        if value.is_some() {
            transaction.execute(
                "UPDATE translations SET accessed_at = ?2 WHERE cache_key = ?1",
                params![key, now],
            )?;
        }
        transaction.commit()?;
        Ok(value)
    }

    pub fn put(&mut self, key: &[u8], value: &str) -> Result<()> {
        let size = value.len() as i64 + key.len() as i64;
        if size as u64 > self.max_bytes {
            return Ok(());
        }
        let now = timestamp();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO translations(cache_key, translated, size_bytes, accessed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(cache_key) DO UPDATE SET
                 translated = excluded.translated,
                 size_bytes = excluded.size_bytes,
                 accessed_at = excluded.accessed_at",
            params![key, value, size, now],
        )?;
        let mut total: i64 = transaction.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM translations",
            [],
            |row| row.get(0),
        )?;
        while total > self.max_bytes as i64 {
            let removed: Option<i64> = transaction
                .query_row(
                    "SELECT size_bytes FROM translations ORDER BY accessed_at ASC, rowid ASC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(removed) = removed else { break };
            transaction.execute(
                "DELETE FROM translations WHERE rowid = (
                    SELECT rowid FROM translations ORDER BY accessed_at ASC, rowid ASC LIMIT 1
                 )",
                [],
            )?;
            total = total.saturating_sub(removed);
        }
        transaction.commit()?;
        Ok(())
    }
}

fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_oldest_entry() {
        let mut cache = Cache::open(Path::new(":memory:"), 75).unwrap();
        let first = Cache::key(&["one", "m", "zh", "p"]);
        let second = Cache::key(&["two", "m", "zh", "p"]);
        cache.put(&first, "1234567890").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        cache.put(&second, "abcdefghij").unwrap();
        assert!(cache.get(&first).unwrap().is_none());
        assert_eq!(cache.get(&second).unwrap().as_deref(), Some("abcdefghij"));
    }
}
