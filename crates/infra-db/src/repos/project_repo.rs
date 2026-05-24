use crate::{DbError, DbPool, DbResult, ProjectRepository};
use snapshort_domain::prelude::*;
use sqlx::Row;
use tracing::instrument;

#[derive(Clone)]
pub struct SqliteProjectRepo {
    pool: DbPool,
}

impl SqliteProjectRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    async fn load_timeline_ids(&self, project_id: uuid::Uuid) -> DbResult<Vec<TimelineId>> {
        let rows = sqlx::query("SELECT id FROM timelines WHERE project_id = ?")
            .bind(project_id.to_string())
            .fetch_all(self.pool.pool())
            .await?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            let id_str: String = row.get("id");
            ids.push(TimelineId(
                uuid::Uuid::parse_str(&id_str)
                    .map_err(|e| DbError::Constraint(format!("Invalid timeline UUID: {e}")))?,
            ));
        }
        Ok(ids)
    }

    async fn load_asset_ids(&self, project_id: uuid::Uuid) -> DbResult<Vec<AssetId>> {
        let rows = sqlx::query("SELECT id FROM assets WHERE project_id = ?")
            .bind(project_id.to_string())
            .fetch_all(self.pool.pool())
            .await?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            let id_str: String = row.get("id");
            ids.push(AssetId(
                uuid::Uuid::parse_str(&id_str)
                    .map_err(|e| DbError::Constraint(format!("Invalid asset UUID: {e}")))?,
            ));
        }
        Ok(ids)
    }
}

impl ProjectRepository for SqliteProjectRepo {
    #[instrument(skip(self, project))]
    async fn create(&self, project: &Project) -> DbResult<()> {
        let settings_json = serde_json::to_string(&project.settings)?;

        sqlx::query(
            r#"
            INSERT INTO projects (id, name, path, settings_json, active_timeline_id, created_at, modified_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(project.id.0.to_string())
        .bind(&project.name)
        .bind(
            project
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        )
        .bind(&settings_json)
        .bind(project.active_timeline_id.map(|id| id.0.to_string()))
        .bind(project.created_at.to_rfc3339())
        .bind(project.modified_at.to_rfc3339())
        .execute(self.pool.pool())
        .await?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn get(&self, id: ProjectId) -> DbResult<Option<Project>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, path, settings_json, active_timeline_id, created_at, modified_at
            FROM projects WHERE id = ?
            "#,
        )
        .bind(id.0.to_string())
        .fetch_optional(self.pool.pool())
        .await?;

        let id_str: String = match row {
            Some(ref row) => row.get("id"),
            None => return Ok(None),
        };

        let timeline_ids = self.load_timeline_ids(uuid::Uuid::parse_str(&id_str)
            .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?).await?;
        let asset_ids = self.load_asset_ids(uuid::Uuid::parse_str(&id_str)
            .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?).await?;

        let row = row.unwrap();
        let settings_json: String = row.get("settings_json");
        let created_str: String = row.get("created_at");
        let modified_str: String = row.get("modified_at");
        let path_opt: Option<String> = row.get("path");
        let active_timeline_id_str: Option<String> = row.get("active_timeline_id");

        Ok(Some(Project {
            id: ProjectId(
                uuid::Uuid::parse_str(&id_str)
                    .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?,
            ),
            name: row.get("name"),
            path: path_opt.map(std::path::PathBuf::from),
            settings: serde_json::from_str(&settings_json)?,
            created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
                .map_err(|e| DbError::Constraint(format!("Invalid date: {}", e)))?
                .with_timezone(&chrono::Utc),
            modified_at: chrono::DateTime::parse_from_rfc3339(&modified_str)
                .map_err(|e| DbError::Constraint(format!("Invalid date: {}", e)))?
                .with_timezone(&chrono::Utc),
            asset_ids,
            timeline_ids,
            active_timeline_id: active_timeline_id_str
                .and_then(|s| uuid::Uuid::parse_str(&s).ok())
                .map(TimelineId),
        }))
    }

    #[instrument(skip(self))]
    async fn get_all(&self) -> DbResult<Vec<Project>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, path, settings_json, active_timeline_id, created_at, modified_at
            FROM projects ORDER BY modified_at DESC
            "#,
        )
        .fetch_all(self.pool.pool())
        .await?;

        let mut projects = Vec::new();
        for row in rows {
            let id_str: String = row.get("id");
            let id = uuid::Uuid::parse_str(&id_str)
                .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?;
            let settings_json: String = row.get("settings_json");
            let created_str: String = row.get("created_at");
            let modified_str: String = row.get("modified_at");
            let path_opt: Option<String> = row.get("path");
            let active_timeline_id_str: Option<String> = row.get("active_timeline_id");

            projects.push(Project {
                id: ProjectId(id),
                name: row.get("name"),
                path: path_opt.map(std::path::PathBuf::from),
                settings: serde_json::from_str(&settings_json)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
                    .map_err(|e| DbError::Constraint(format!("Invalid date: {}", e)))?
                    .with_timezone(&chrono::Utc),
                modified_at: chrono::DateTime::parse_from_rfc3339(&modified_str)
                    .map_err(|e| DbError::Constraint(format!("Invalid date: {}", e)))?
                    .with_timezone(&chrono::Utc),
                asset_ids: self.load_asset_ids(id).await?,
                timeline_ids: self.load_timeline_ids(id).await?,
                active_timeline_id: active_timeline_id_str
                    .and_then(|s| uuid::Uuid::parse_str(&s).ok())
                    .map(TimelineId),
            });
        }

        Ok(projects)
    }

    #[instrument(skip(self, project))]
    async fn update(&self, project: &Project) -> DbResult<()> {
        let settings_json = serde_json::to_string(&project.settings)?;

        let result = sqlx::query(
            r#"
            UPDATE projects 
            SET name = ?, path = ?, settings_json = ?, active_timeline_id = ?, modified_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&project.name)
        .bind(
            project
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        )
        .bind(&settings_json)
        .bind(project.active_timeline_id.map(|id| id.0.to_string()))
        .bind(project.modified_at.to_rfc3339())
        .bind(project.id.0.to_string())
        .execute(self.pool.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Project",
                id: project.id.0,
            });
        }

        Ok(())
    }

    #[instrument(skip(self))]
    async fn delete(&self, id: ProjectId) -> DbResult<()> {
        let result = sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id.0.to_string())
            .execute(self.pool.pool())
            .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Project",
                id: id.0,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> DbPool {
        DbPool::in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn test_create_and_get_project() {
        let pool = test_pool().await;
        let repo = SqliteProjectRepo::new(pool);

        let project = Project::new("Test Project");
        repo.create(&project).await.unwrap();

        let loaded = repo.get(project.id).await.unwrap().unwrap();
        assert_eq!(loaded.name, "Test Project");
    }

    #[tokio::test]
    async fn test_update_project() {
        let pool = test_pool().await;
        let repo = SqliteProjectRepo::new(pool);

        let mut project = Project::new("Original");
        repo.create(&project).await.unwrap();

        project.name = "Updated".to_string();
        project.touch();
        repo.update(&project).await.unwrap();

        let loaded = repo.get(project.id).await.unwrap().unwrap();
        assert_eq!(loaded.name, "Updated");
    }
}
