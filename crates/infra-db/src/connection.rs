use crate::DbResult;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};
use std::path::Path;
use std::str::FromStr;
use tracing::info;

#[derive(Clone)]
pub struct DbPool {
    pool: Pool<Sqlite>,
}

impl DbPool {
    pub async fn new(path: impl AsRef<Path>) -> DbResult<Self> {
        let path = path.as_ref();
        info!("Opening database at: {}", path.display());

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(30));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;

        Ok(db)
    }

    pub async fn in_memory() -> DbResult<Self> {
        let options = SqliteConnectOptions::from_str(":memory:")?
            .journal_mode(SqliteJournalMode::Memory)
            .shared_cache(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;

        Ok(db)
    }

    async fn run_migrations(&self) -> DbResult<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        info!("Database migrations completed");
        Ok(())
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub async fn begin(&self) -> DbResult<sqlx::Transaction<'_, Sqlite>> {
        Ok(self.pool.begin().await?)
    }
}

impl std::ops::Deref for DbPool {
    type Target = Pool<Sqlite>;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}
