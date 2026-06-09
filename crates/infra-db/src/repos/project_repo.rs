use crate::{DbConn, DbError, DbResult};
use miniter_domain::{Project, ProjectId};
use sqlx::Row;

#[derive(Clone)]
pub struct ProjectRepo {
    conn: DbConn,
}

impl ProjectRepo {
    pub fn new(conn: DbConn) -> Self {
        Self { conn }
    }

    pub async fn create(&self, project: &Project) -> DbResult<()> {
        let json = project.to_json()?;
        let now = project.meta.modified_at;
        sqlx::query(
            r#"INSERT INTO projects (id, name, project_json, created_at, modified_at) VALUES (?, ?, ?, ?, ?)"#,
        )
        .bind(project.id.0.to_string())
        .bind(&project.meta.name)
        .bind(&json)
        .bind(now)
        .bind(now)
        .execute(self.conn.pool())
        .await?;
        Ok(())
    }

    pub async fn get(&self, id: ProjectId) -> DbResult<Option<Project>> {
        let row = sqlx::query("SELECT project_json FROM projects WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(self.conn.pool())
            .await?;

        match row {
            Some(row) => {
                let json: String = row.get("project_json");
                Ok(Some(Project::from_json(&json)?))
            }
            None => Ok(None),
        }
    }

    pub async fn get_all(&self) -> DbResult<Vec<Project>> {
        let rows = sqlx::query("SELECT project_json FROM projects ORDER BY modified_at DESC")
            .fetch_all(self.conn.pool())
            .await?;

        let mut projects = Vec::new();
        for row in rows {
            let json: String = row.get("project_json");
            projects.push(Project::from_json(&json)?);
        }
        Ok(projects)
    }

    pub async fn update(&self, project: &Project) -> DbResult<()> {
        let json = project.to_json()?;
        let result = sqlx::query(
            r#"UPDATE projects SET project_json = ?, modified_at = ? WHERE id = ?"#,
        )
        .bind(&json)
        .bind(project.meta.modified_at)
        .bind(project.id.0.to_string())
        .execute(self.conn.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Project",
                id: project.id.0,
            });
        }
        Ok(())
    }

    pub async fn delete(&self, id: ProjectId) -> DbResult<()> {
        let result = sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id.0.to_string())
            .execute(self.conn.pool())
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
    use crate::DbConn;

    async fn test_conn() -> DbConn {
        DbConn::in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn test_create_and_get_project() {
        let conn = test_conn().await;
        let repo = ProjectRepo::new(conn);
        let project = Project::new("Test Project");
        repo.create(&project).await.unwrap();
        let loaded = repo.get(project.id).await.unwrap().unwrap();
        assert_eq!(loaded.meta.name, "Test Project");
    }
}
