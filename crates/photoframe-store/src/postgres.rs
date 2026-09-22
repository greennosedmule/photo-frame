use sqlx::pool::PoolConnection;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres};
use tokio::sync::Mutex;

use crate::{Error, IndexerLock, Result};

/// Arbitrary application-specific advisory lock key ("PHOTOFRM").
const INDEXER_LOCK_KEY: i64 = 0x5048_4f54_4f46_524d;

pub(crate) struct PostgresStore {
    pool: PgPool,
    /// Advisory locks are session-scoped, so the connection that took the lock
    /// must stay checked out for as long as the lock is held.
    lock_conn: Mutex<Option<PoolConnection<Postgres>>>,
}

impl PostgresStore {
    pub(crate) async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;
        Ok(Self {
            pool,
            lock_conn: Mutex::new(None),
        })
    }
}

impl PostgresStore {
    async fn begin_tx(&self) -> Result<sqlx::Transaction<'static, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }
}

/// Rewrite `?` placeholders as `$1, $2, ...`.
fn ph(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0;
    for c in sql.chars() {
        if c == '?' {
            n += 1;
            out.push('$');
            out.push_str(&n.to_string());
        } else {
            out.push(c);
        }
    }
    out
}

fn ts_in() -> &'static str {
    "CAST(? AS TIMESTAMPTZ)"
}

fn ts_out(col: &str) -> String {
    format!("to_char({col} AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')")
}

impl_store! { PostgresStore, sqlx::Postgres;
    async fn migrate(&self) -> Result<()> {
        // sqlx takes its own advisory lock, so concurrent starts are safe.
        sqlx::migrate!("../../migrations/postgres")
            .run(&self.pool)
            .await?;
        Ok(())
    }

    async fn generation(&self) -> Result<i64> {
        let v: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = 'generation'")
            .fetch_one(&self.pool)
            .await?;
        Ok(v.parse().unwrap_or(0))
    }

    async fn bump_generation(&self) -> Result<i64> {
        let v: String = sqlx::query_scalar(
            "UPDATE meta SET value = (value::bigint + 1)::text \
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
        let mut conn = self.pool.acquire().await?;
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(INDEXER_LOCK_KEY)
            .fetch_one(&mut *conn)
            .await?;
        if !got {
            return Err(Error::LockHeld);
        }
        *self.lock_conn.lock().await = Some(conn);
        Ok(IndexerLock::new())
    }

    async fn release_indexer_lock(&self, _lock: IndexerLock) -> Result<()> {
        if let Some(mut conn) = self.lock_conn.lock().await.take() {
            sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(INDEXER_LOCK_KEY)
                .execute(&mut *conn)
                .await?;
        }
        Ok(())
    }
}
