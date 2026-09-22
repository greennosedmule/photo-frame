pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("DATABASE_URL must start with sqlite:// or postgres://")]
    UnsupportedUrl,
    #[error("another indexer instance holds the singleton lock")]
    LockHeld,
    #[error("invalid value: {0}")]
    Invalid(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}
