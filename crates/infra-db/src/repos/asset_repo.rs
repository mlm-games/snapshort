use crate::{DbPool, DbResult, DbError, AssetRepository};
use snapshort_domain::prelude::*;
use sqlx::Row;
use std::path::PathBuf;
use tracing::instrument;

#[derive(Clone)]
pub struct SqliteAssetRepo {
    pool: DbPool,
}

impl SqliteAssetRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl AssetRepository for SqliteAssetRepo {
    #[instrument(skip(self, asset))]
    async fn create(&self, project_id: ProjectId, asset: &Asset) -> DbResult<()> {
        let media_info_json = asset.media_info.as_ref()
            .map(|m| serde_json::to_string(m))
            .transpose()?;
        let proxy_json = asset.proxy.as_ref()
            .map(|p| serde_json::to_string(p))
            .transpose()?;
        let tags_json = serde_json::to_string(&asset.tags)?;
        let markers_json = serde_json::to_string(&asset.markers)?;
        let status_str = status_to_string(&asset.status);
        let asset_type_str = asset_type_to_string(&asset.asset_type);

        sqlx::query(
            r#"
            INSERT INTO assets (
                id, project_id, name, path, asset_type, status,
                media_info_json, proxy_json, imported_at, modified_at,
                tags_json, notes, rating, markers_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(asset.id.0.to_string())
        .bind(project_id.0.to_string())
        .bind(&asset.name)
        .bind(asset.path.to_string_lossy().to_string())
        .bind(asset_type_str)
        .bind(status_str)
        .bind(&media_info_json)
        .bind(&proxy_json)
        .bind(asset.imported_at.to_rfc3339())
        .bind(asset.modified_at.to_rfc3339())
        .bind(&tags_json)
        .bind(&asset.notes)
        .bind(asset.rating.map(|r| r as i32))
        .bind(&markers_json)
        .execute(self.pool.pool())
        .await?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn get(&self, id: AssetId) -> DbResult<Option<Asset>> {
        let row = sqlx::query("SELECT * FROM assets WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(self.pool.pool())
            .await?;

        match row {
            Some(row) => Ok(Some(row_to_asset(&row)?)),
            None => Ok(None),
        }
    }

    #[instrument(skip(self))]
    async fn get_by_project(&self, project_id: ProjectId) -> DbResult<Vec<Asset>> {
        let rows = sqlx::query(
            "SELECT * FROM assets WHERE project_id = ? ORDER BY imported_at DESC"
        )
        .bind(project_id.0.to_string())
        .fetch_all(self.pool.pool())
        .await?;

        rows.iter().map(row_to_asset).collect()
    }

    #[instrument(skip(self, asset))]
    async fn update(&self, asset: &Asset) -> DbResult<()> {
        let media_info_json = asset.media_info.as_ref()
            .map(|m| serde_json::to_string(m))
            .transpose()?;
        let proxy_json = asset.proxy.as_ref()
            .map(|p| serde_json::to_string(p))
            .transpose()?;
        let tags_json = serde_json::to_string(&asset.tags)?;
        let markers_json = serde_json::to_string(&asset.markers)?;
        let status_str = status_to_string(&asset.status);

        let result = sqlx::query(
            r#"
            UPDATE assets SET
                name = ?, status = ?, media_info_json = ?, proxy_json = ?,
                modified_at = ?, tags_json = ?, notes = ?, rating = ?, markers_json = ?
            WHERE id = ?
            "#
        )
        .bind(&asset.name)
        .bind(status_str)
        .bind(&media_info_json)
        .bind(&proxy_json)
        .bind(asset.modified_at.to_rfc3339())
        .bind(&tags_json)
        .bind(&asset.notes)
        .bind(asset.rating.map(|r| r as i32))
        .bind(&markers_json)
        .bind(asset.id.0.to_string())
        .execute(self.pool.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Asset",
                id: asset.id.0,
            });
        }

        Ok(())
    }

    #[instrument(skip(self))]
    async fn delete(&self, id: AssetId) -> DbResult<()> {
        let result = sqlx::query("DELETE FROM assets WHERE id = ?")
            .bind(id.0.to_string())
            .execute(self.pool.pool())
            .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Asset",
                id: id.0,
            });
        }

        Ok(())
    }

    #[instrument(skip(self))]
    async fn update_status(&self, id: AssetId, status: AssetStatus) -> DbResult<()> {
        let status_str = status_to_string(&status);
        let now = chrono::Utc::now().to_rfc3339();

        let result = sqlx::query(
            "UPDATE assets SET status = ?, modified_at = ? WHERE id = ?"
        )
        .bind(status_str)
        .bind(&now)
        .bind(id.0.to_string())
        .execute(self.pool.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Asset",
                id: id.0,
            });
        }

        Ok(())
    }
}

fn row_to_asset(row: &sqlx::sqlite::SqliteRow) -> DbResult<Asset> {
    let id_str: String = row.get("id");
    let imported_str: String = row.get("imported_at");
    let modified_str: String = row.get("modified_at");
    let media_info_json: Option<String> = row.get("media_info_json");
    let proxy_json: Option<String> = row.get("proxy_json");
    let tags_json: String = row.get("tags_json");
    let markers_json: String = row.get("markers_json");
    let status_str: String = row.get("status");
    let asset_type_str: String = row.get("asset_type");
    let path_str: String = row.get("path");

    Ok(Asset {
        id: AssetId(uuid::Uuid::parse_str(&id_str)
            .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?),
        name: row.get("name"),
        path: PathBuf::from(path_str),
        asset_type: string_to_asset_type(&asset_type_str)?,
        status: string_to_status(&status_str)?,
        media_info: media_info_json.map(|s| serde_json::from_str(&s)).transpose()?,
        proxy: proxy_json.map(|s| serde_json::from_str(&s)).transpose()?,
        imported_at: chrono::DateTime::parse_from_rfc3339(&imported_str)
            .map_err(|e| DbError::Constraint(format!("Invalid date: {}", e)))?
            .with_timezone(&chrono::Utc),
        modified_at: chrono::DateTime::parse_from_rfc3339(&modified_str)
            .map_err(|e| DbError::Constraint(format!("Invalid date: {}", e)))?
            .with_timezone(&chrono::Utc),
        tags: serde_json::from_str(&tags_json)?,
        notes: row.get("notes"),
        rating: row.get::<Option<i32>, _>("rating").map(|r| r as u8),
        markers: serde_json::from_str(&markers_json)?,
    })
}

fn status_to_string(status: &AssetStatus) -> String {
    match status {
        AssetStatus::Pending => "pending".to_string(),
        AssetStatus::Analyzing => "analyzing".to_string(),
        AssetStatus::Ready => "ready".to_string(),
        AssetStatus::ProxyGenerating { progress } => format!("proxy_generating:{}", progress),
        AssetStatus::ProxyReady => "proxy_ready".to_string(),
        AssetStatus::Error(e) => format!("error:{}", e),
        AssetStatus::Offline => "offline".to_string(),
    }
}

fn string_to_status(s: &str) -> DbResult<AssetStatus> {
    Ok(match s {
        "pending" => AssetStatus::Pending,
        "analyzing" => AssetStatus::Analyzing,
        "ready" => AssetStatus::Ready,
        "proxy_ready" => AssetStatus::ProxyReady,
        "offline" => AssetStatus::Offline,
        s if s.starts_with("proxy_generating:") => {
            let progress: u8 = s.strip_prefix("proxy_generating:")
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);
            AssetStatus::ProxyGenerating { progress }
        }
        s if s.starts_with("error:") => {
            AssetStatus::Error(s.strip_prefix("error:").unwrap_or("Unknown").to_string())
        }
        _ => AssetStatus::Pending,
    })
}

fn asset_type_to_string(t: &AssetType) -> &'static str {
    match t {
        AssetType::Video => "video",
        AssetType::Audio => "audio",
        AssetType::Image => "image",
        AssetType::Sequence => "sequence",
    }
}

fn string_to_asset_type(s: &str) -> DbResult<AssetType> {
    Ok(match s {
        "video" => AssetType::Video,
        "audio" => AssetType::Audio,
        "image" => AssetType::Image,
        "sequence" => AssetType::Sequence,
        _ => return Err(DbError::Constraint(format!("Unknown asset type: {}", s))),
    })
}
