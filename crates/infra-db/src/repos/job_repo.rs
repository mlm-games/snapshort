use crate::{DbError, DbPool, DbResult};
use chrono::{DateTime, Utc};
use sqlx::{sqlite::SqliteRow, Row};
use tracing::instrument;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct JobRow {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub progress: Option<i32>,
    pub payload_json: String,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SqliteJobRepo {
    pool: DbPool,
}

impl SqliteJobRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    #[instrument(skip(self))]
    pub async fn create(&self, id: Uuid, kind: &str, payload_json: &str) -> DbResult<()> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO jobs (id, kind, status, progress, payload_json, result_json, error, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id.to_string())
        .bind(kind)
        .bind("queued")
        .bind(None::<i32>)
        .bind(payload_json)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(&now)
        .bind(&now)
        .execute(self.pool.pool())
        .await?;

        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn get(&self, id: Uuid) -> DbResult<Option<JobRow>> {
        let row = sqlx::query(
            r#"
            SELECT id, kind, status, progress, payload_json, result_json, error, created_at, updated_at
            FROM jobs
            WHERE id = ?
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool.pool())
        .await?;

        Ok(row.map(|row: SqliteRow| row_to_job(&row)).transpose()?)
    }

    #[instrument(skip(self))]
    pub async fn list_pending(&self) -> DbResult<Vec<JobRow>> {
        let rows = sqlx::query(
            r#"
            SELECT id, kind, status, progress, payload_json, result_json, error, created_at, updated_at
            FROM jobs
            WHERE status IN ('queued', 'running')
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(self.pool.pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(row_to_job(&r)?);
        }
        Ok(out)
    }

    /// Recovery rule: anything "running" becomes "queued" on startup.
    #[instrument(skip(self))]
    pub async fn recover_incomplete(&self) -> DbResult<u64> {
        let now = Utc::now().to_rfc3339();
        let res = sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'queued', updated_at = ?
            WHERE status = 'running'
            "#,
        )
        .bind(&now)
        .execute(self.pool.pool())
        .await?;

        Ok(res.rows_affected())
    }

    #[instrument(skip(self))]
    pub async fn set_running(&self, id: Uuid) -> DbResult<()> {
        self.set_status(id, "running", None, None, None).await
    }

    #[instrument(skip(self))]
    pub async fn set_progress(&self, id: Uuid, progress: u8) -> DbResult<()> {
        self.set_status(id, "running", Some(progress as i32), None, None)
            .await
    }

    #[instrument(skip(self))]
    pub async fn set_succeeded(&self, id: Uuid, result_json: Option<String>) -> DbResult<()> {
        self.set_status(id, "succeeded", Some(100), result_json, None)
            .await
    }

    #[instrument(skip(self))]
    pub async fn set_failed(&self, id: Uuid, error: String) -> DbResult<()> {
        self.set_status(id, "failed", None, None, Some(error)).await
    }

    #[instrument(skip(self))]
    pub async fn set_canceled(&self, id: Uuid) -> DbResult<()> {
        self.set_status(id, "canceled", None, None, None).await
    }

    async fn set_status(
        &self,
        id: Uuid,
        status: &str,
        progress: Option<i32>,
        result_json: Option<String>,
        error: Option<String>,
    ) -> DbResult<()> {
        let now = Utc::now().to_rfc3339();

        let res = sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?, progress = ?, result_json = ?, error = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(progress)
        .bind(result_json)
        .bind(error)
        .bind(&now)
        .bind(id.to_string())
        .execute(self.pool.pool())
        .await?;

        if res.rows_affected() == 0 {
            // Your DbError::NotFound expects `entity_type` (per your error)
            return Err(DbError::NotFound {
                entity_type: "job",
                id,
            });
        }

        Ok(())
    }
}

fn row_to_job(row: &sqlx::sqlite::SqliteRow) -> DbResult<JobRow> {
    let id_str: String = row.get("id");
    let created_str: String = row.get("created_at");
    let updated_str: String = row.get("updated_at");

    Ok(JobRow {
        id: Uuid::parse_str(&id_str)
            .map_err(|e| DbError::Constraint(format!("Invalid UUID: {e}")))?,
        kind: row.get("kind"),
        status: row.get("status"),
        progress: row.get("progress"),
        payload_json: row.get("payload_json"),
        result_json: row.get("result_json"),
        error: row.get("error"),
        created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
            .map_err(|e| DbError::Constraint(format!("Invalid date: {e}")))?
            .with_timezone(&Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&updated_str)
            .map_err(|e| DbError::Constraint(format!("Invalid date: {e}")))?
            .with_timezone(&Utc),
    })
}
