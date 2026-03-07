use crate::{AppEvent, AppResult, EventBus};
use snapshort_domain::prelude::*;
use snapshort_infra_db::{
    repos::{asset_repo::SqliteAssetRepo, job_repo::SqliteJobRepo},
    AssetRepository, DbPool,
};
use snapshort_infra_media::MediaEngine;

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, Semaphore},
    task::spawn_blocking,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum JobSpec {
    AnalyzeAsset { asset_id: AssetId },
    GenerateProxy { asset_id: AssetId },
}

#[derive(Clone)]
pub struct JobsService {
    _db: DbPool,
    job_repo: SqliteJobRepo,
    asset_repo: SqliteAssetRepo,
    event_bus: EventBus,
    proxy_dir: PathBuf,

    media: Arc<MediaEngine>,

    // lanes
    sem_analyze: Arc<Semaphore>,
    sem_proxy: Arc<Semaphore>,
    active: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
}

impl JobsService {
    pub fn new(db: DbPool, event_bus: EventBus, proxy_dir: PathBuf) -> Self {
        Self {
            _db: db.clone(),
            job_repo: SqliteJobRepo::new(db.clone()),
            asset_repo: SqliteAssetRepo::new(db.clone()),
            event_bus,
            proxy_dir,
            media: Arc::new(MediaEngine::default()),
            sem_analyze: Arc::new(Semaphore::new(4)),
            sem_proxy: Arc::new(Semaphore::new(2)),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Startup recovery: "running" -> "queued", then re-spawn pending work.
    #[instrument(skip(self))]
    pub async fn recover_and_resume(&self) -> AppResult<()> {
        let recovered = self.job_repo.recover_incomplete().await?;
        if recovered > 0 {
            info!("Recovered {recovered} running jobs -> queued");
        }

        let pending = self.job_repo.list_pending().await?;
        for row in pending {
            if row.status != "queued" {
                continue;
            }
            let spec: JobSpec = serde_json::from_str(&row.payload_json)?;
            self.spawn_existing(row.id, spec).await?;
        }
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn submit(&self, spec: JobSpec) -> AppResult<Uuid> {
        let id = Uuid::new_v4();
        let (kind, payload_json) = kind_and_payload(&spec)?;

        self.job_repo.create(id, &kind, &payload_json).await?;
        self.event_bus.emit(AppEvent::JobQueued {
            job_id: id,
            kind: kind.clone(),
        });

        self.spawn_existing(id, spec).await?;
        Ok(id)
    }

    #[instrument(skip(self))]
    pub async fn cancel(&self, job_id: Uuid) -> AppResult<()> {
        let mut active = self.active.lock().await;
        if let Some(token) = active.remove(&job_id) {
            token.cancel();
            self.job_repo.set_canceled(job_id).await.ok();
            self.event_bus.emit(AppEvent::JobCanceled { job_id });
        }
        Ok(())
    }

    async fn spawn_existing(&self, job_id: Uuid, spec: JobSpec) -> AppResult<()> {
        let token = CancellationToken::new();
        self.active.lock().await.insert(job_id, token.clone());

        let me = self.clone();
        tokio::spawn(async move {
            let _ = me.run_job(job_id, spec, token).await;
            me.active.lock().await.remove(&job_id);
        });

        Ok(())
    }

    async fn run_job(
        &self,
        job_id: Uuid,
        spec: JobSpec,
        cancel: CancellationToken,
    ) -> AppResult<()> {
        self.event_bus.emit(AppEvent::JobStarted { job_id });
        self.job_repo.set_running(job_id).await?;

        match spec {
            JobSpec::AnalyzeAsset { asset_id } => {
                let _permit = self
                    .sem_analyze
                    .acquire()
                    .await
                    .map_err(|e| crate::AppError::Other(format!("Analyze lane unavailable: {e}")))?;

                if cancel.is_cancelled() {
                    self.job_repo.set_canceled(job_id).await?;
                    self.event_bus.emit(AppEvent::JobCanceled { job_id });
                    return Ok(());
                }

                self.job_repo.set_progress(job_id, 5).await?;
                self.event_bus.emit(AppEvent::JobProgress {
                    job_id,
                    progress: 5,
                    message: Some("Analyzing…".into()),
                });

                let Some(mut asset) = self.asset_repo.get(asset_id).await? else {
                    self.job_repo
                        .set_failed(job_id, format!("Asset not found: {asset_id}"))
                        .await?;
                    self.event_bus.emit(AppEvent::JobFailed {
                        job_id,
                        error: "Asset not found".into(),
                    });
                    return Ok(());
                };

                asset.status = AssetStatus::Analyzing;
                asset.touch();
                self.asset_repo.update(&asset).await?;
                self.event_bus.emit(AppEvent::AssetUpdated {
                    asset: asset.clone(),
                });

                // Do probe off-thread
                let media = self.media.clone();
                let path = asset.path.clone();
                let info = tokio::task::spawn_blocking(move || media.probe(&path))
                    .await
                    .map_err(|e| crate::AppError::Other(format!("Join error: {e}")))?
                    .map_err(|e| crate::AppError::Other(format!("Media probe failed: {e}")))?;

                self.job_repo.set_progress(job_id, 80).await?;
                self.event_bus.emit(AppEvent::JobProgress {
                    job_id,
                    progress: 80,
                    message: Some("Finalizing…".into()),
                });

                asset.media_info = Some(info);
                asset.status = AssetStatus::Ready;
                asset.touch();
                self.asset_repo.update(&asset).await?;

                self.event_bus.emit(AppEvent::AssetAnalyzed {
                    asset: asset.clone(),
                });
                self.job_repo.set_succeeded(job_id, None).await?;
                self.event_bus.emit(AppEvent::JobFinished { job_id });
                Ok(())
            }

            JobSpec::GenerateProxy { asset_id } => {
                let _permit = self
                    .sem_proxy
                    .acquire()
                    .await
                    .map_err(|e| crate::AppError::Other(format!("Proxy lane unavailable: {e}")))?;

                let Some(mut asset) = self.asset_repo.get(asset_id).await? else {
                    self.job_repo
                        .set_failed(job_id, format!("Asset not found: {asset_id}"))
                        .await?;
                    self.event_bus.emit(AppEvent::JobFailed {
                        job_id,
                        error: "Asset not found".into(),
                    });
                    return Ok(());
                };

                asset.status = AssetStatus::ProxyGenerating { progress: 0 };
                asset.touch();
                self.asset_repo.update(&asset).await?;
                self.event_bus.emit(AppEvent::AssetUpdated {
                    asset: asset.clone(),
                });

                std::fs::create_dir_all(&self.proxy_dir).ok();

                for p in (0..=100u8).step_by(20) {
                    if cancel.is_cancelled() {
                        self.job_repo.set_canceled(job_id).await?;
                        self.event_bus.emit(AppEvent::JobCanceled { job_id });

                        let _ = self
                            .asset_repo
                            .update_status(asset_id, AssetStatus::Error("Proxy canceled".into()))
                            .await;
                        return Ok(());
                    }

                    let _ = self
                        .asset_repo
                        .update_status(asset_id, AssetStatus::ProxyGenerating { progress: p })
                        .await;

                    self.job_repo.set_progress(job_id, p).await?;
                    self.event_bus.emit(AppEvent::JobProgress {
                        job_id,
                        progress: p,
                        message: Some(format!("Proxy {p}%")),
                    });
                    self.event_bus.emit(AppEvent::AssetProxyProgress {
                        asset_id,
                        progress: p,
                    });

                    sleep(Duration::from_millis(80)).await;
                }

                // Generate a placeholder proxy (off-thread; later swap to real ffmpeg)
                let media = self.media.clone();
                let out_dir = self.proxy_dir.clone();
                let asset_uuid = asset.id.0;
                let input_path = asset.path.clone();
                let proxy = spawn_blocking(move || media.create_proxy(asset_uuid, &input_path, &out_dir))
                    .await
                    .map_err(|e| crate::AppError::Other(format!("Join error: {e}")))?
                    .map_err(|e| crate::AppError::Other(format!("Proxy generation failed: {e}")))?;

                asset.proxy = Some(proxy);
                asset.status = AssetStatus::ProxyReady;
                asset.touch();
                self.asset_repo.update(&asset).await?;

                self.event_bus.emit(AppEvent::AssetProxyComplete {
                    asset: asset.clone(),
                });
                self.job_repo.set_succeeded(job_id, None).await?;
                self.event_bus.emit(AppEvent::JobFinished { job_id });
                Ok(())
            }
        }
    }
}

fn kind_and_payload(spec: &JobSpec) -> AppResult<(String, String)> {
    let kind = match spec {
        JobSpec::AnalyzeAsset { .. } => "analyze_asset".to_string(),
        JobSpec::GenerateProxy { .. } => "generate_proxy".to_string(),
    };
    let payload_json = serde_json::to_string(spec)?;
    Ok((kind, payload_json))
}
