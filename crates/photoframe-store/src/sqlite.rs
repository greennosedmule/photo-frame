use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::{Error, IndexerLock, Result};

pub(crate) struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    pub(crate) async fn connect(url: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            // WAL: one writer, many readers, across two processes in one pod.
            // Never point this at NFS.
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(10));
        // A `:memory:` database is per-connection, so tests must use one connection.
        let max = if url.contains(":memory:") { 1 } else { 5 };
        // The first open of a new file switches it to WAL, which can report
        // "database is locked" when two processes start together; retry.
        let mut attempt = 0;
        let pool = loop {
            match SqlitePoolOptions::new()
                .max_connections(max)
                .connect_with(opts.clone())
                .await
            {
                Ok(pool) => break pool,
                Err(_) if attempt < 5 => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(100 * attempt)).await;
                }
                Err(e) => return Err(e.into()),
            }
        };
        Ok(Self { pool })
    }
}

impl SqliteStore {
    /// Write transactions take the write lock up front. A DEFERRED transaction
    /// that reads and then writes fails at once with SQLITE_BUSY when another
    /// writer got in between, and the busy timeout cannot help with that.
    async fn begin_tx(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
    }
}

fn ph(sql: &str) -> String {
    sql.to_string()
}

fn ts_in() -> &'static str {
    "?"
}

fn ts_out(col: &str) -> String {
    col.to_string()
}

impl_store! { SqliteStore, sqlx::Sqlite;
    async fn migrate(&self) -> Result<()> {
        // sqlx takes no lock on SQLite, so two processes starting together can
        // both try to record the same migration and one hits a UNIQUE violation.
        // Retrying is enough: the loser then sees the winner's applied rows, and
        // the migrations themselves are idempotent.
        let migrator = sqlx::migrate!("../../migrations/sqlite");
        let mut attempt = 0;
        loop {
            match migrator.run(&self.pool).await {
                Ok(()) => return Ok(()),
                Err(_) if attempt < 5 => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(100 * attempt)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    async fn generation(&self) -> Result<i64> {
        let v: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = 'generation'")
            .fetch_one(&self.pool)
            .await?;
        Ok(v.parse().unwrap_or(0))
    }

    async fn bump_generation(&self) -> Result<i64> {
        let v: String = sqlx::query_scalar(
            "UPDATE meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) \
             WHERE key = 'generation' RETURNING value",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(v.parse().unwrap_or(0))
    }

    async fn request_scan(&self) -> Result<()> {
        sqlx::query("UPDATE meta SET value = '1' WHERE key = 'scan_requested'")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn take_scan_request(&self) -> Result<bool> {
        let n = sqlx::query(
            "UPDATE meta SET value = '0' WHERE key = 'scan_requested' AND value <> '0'",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n > 0)
    }

    async fn acquire_indexer_lock(&self) -> Result<IndexerLock> {
        let holder = format!("pid:{}", std::process::id());
        let res = sqlx::query(
            "INSERT INTO singleton (id, holder, acquired_at) \
             VALUES (1, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) ON CONFLICT DO NOTHING",
        )
        .bind(holder)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(Error::LockHeld);
        }
        Ok(IndexerLock::new())
    }

    async fn release_indexer_lock(&self, _lock: IndexerLock) -> Result<()> {
        sqlx::query("DELETE FROM singleton WHERE id = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
