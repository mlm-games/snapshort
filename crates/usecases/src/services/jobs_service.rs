use crate::{AppEvent, AppResult, EventBus};
use snapshort_domain::prelude::*;
use snapshort_infra_db::{
    repos::{asset_repo::SqliteAssetRepo, job_repo::SqliteJobRepo},
    AssetRepository, DbPool,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, Semaphore};
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
    db: DbPool,
    job_repo: SqliteJobRepo,
    asset_repo: SqliteAssetRepo,
    event_bus: EventBus,

    proxy_dir: PathBuf,

    // lanes
    sem_analyze: Arc<Semaphore>,
    sem_proxy: Arc<Semaphore>,

    active: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
}

impl JobsService {
    pub fn new(db: DbPool, event_bus: EventBus, proxy_dir: PathBuf) -> Self {
        Self {
            db: db.clone(),
            job_repo: SqliteJobRepo::new(db.clone()),
            asset_repo: SqliteAssetRepo::new(db.clone()),
            event_bus,
            proxy_dir,
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
        // ensure we can cancel it
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
                let _permit = self.sem_analyze.acquire().await.unwrap();

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

                // Minimal “analysis” placeholder: mark asset Ready if it exists.
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
                self.asset_repo.update(&asset).await?;
                self.event_bus.emit(AppEvent::AssetUpdated {
                    asset: asset.clone(),
                });

                asset.status = AssetStatus::Analyzing;
                self.asset_repo.update(&asset).await?;

                // Stub result: keep your existing stub analyzer elsewhere if you want,
                // but Phase 1 needs orchestration. We'll set Ready.
                asset.status = AssetStatus::Ready;
                self.asset_repo.update(&asset).await?;

                self.event_bus.emit(AppEvent::AssetAnalyzed {
                    asset: asset.clone(),
                });

                self.job_repo.set_succeeded(job_id, None).await?;
                self.event_bus.emit(AppEvent::JobFinished { job_id });
                Ok(())
            }

            JobSpec::GenerateProxy { asset_id } => {
                let _permit = self.sem_proxy.acquire().await.unwrap();

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
                self.asset_repo.update(&asset).await?;

                // Phase 1: stub proxy generation with progress + placeholder file (your old behavior),
                // but now fully job-driven and cancellable.
                std::fs::create_dir_all(&self.proxy_dir).ok();

                for p in (0..=100u8).step_by(20) {
                    if cancel.is_cancelled() {
                        self.job_repo.set_canceled(job_id).await?;
                        self.event_bus.emit(AppEvent::JobCanceled { job_id });
                        asset.status = AssetStatus::Error("Proxy canceled".into());
                        let _ = self.asset_repo.update(&asset).await;
                        return Ok(());
                    }

                    asset.status = AssetStatus::ProxyGenerating { progress: p };
                    let _ = self.asset_repo.update(&asset).await;

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

                    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                }

                // placeholder proxy file
                let proxy_path = self.proxy_dir.join(format!("{}_proxy.mp4", asset.id.0));
                let _ = std::fs::write(&proxy_path, b"proxy placeholder");

                asset.proxy = Some(ProxyInfo {
                    path: proxy_path,
                    codec: "h264".to_string(),
                    resolution: Resolution::HD,
                    fps: Fps::F24,
                    bitrate_kbps: 800,
                    created_at: chrono::Utc::now(),
                });
                asset.status = AssetStatus::ProxyReady;

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
