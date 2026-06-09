use crate::{AppEvent, AppResult, Asset, AssetId, AssetStatus, EventBus};
use snapshort_infra_db::repos::job_repo::SqliteJobRepo;
use snapshort_infra_db::DbConn;
use snapshort_infra_media::MediaEngine;

use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    sync::{Mutex, RwLock, Semaphore},
    task::spawn_blocking,
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
    job_repo: SqliteJobRepo,
    event_bus: EventBus,
    proxy_dir: PathBuf,

    assets: Arc<RwLock<HashMap<AssetId, Asset>>>,
    media: Arc<MediaEngine>,

    sem_analyze: Arc<Semaphore>,
    sem_proxy: Arc<Semaphore>,
    active: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
}

impl JobsService {
    pub fn new(db: DbConn, event_bus: EventBus, proxy_dir: PathBuf) -> Self {
        Self {
            job_repo: SqliteJobRepo::new(db),
            event_bus,
            proxy_dir,
            assets: Arc::new(RwLock::new(HashMap::new())),
            media: Arc::new(MediaEngine::default()),
            sem_analyze: Arc::new(Semaphore::new(4)),
            sem_proxy: Arc::new(Semaphore::new(2)),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn load_assets(&self, assets: Vec<Asset>) {
        let mut store = self.assets.write().await;
        store.clear();
        for asset in assets {
            store.insert(asset.id, asset);
        }
    }

    pub async fn insert_asset(&self, asset: Asset) {
        let mut store = self.assets.write().await;
        store.insert(asset.id, asset);
    }

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
        let spec_for_error = spec.clone();
        tokio::spawn(async move {
            if let Err(e) = me.run_job(job_id, spec, token).await {
                let error = e.to_string();
                let _ = me.job_repo.set_failed(job_id, error.clone()).await;
                let asset_id = match &spec_for_error {
                    JobSpec::AnalyzeAsset { asset_id } => *asset_id,
                    JobSpec::GenerateProxy { asset_id } => *asset_id,
                };
                let mut store = me.assets.write().await;
                if let Some(asset) = store.get_mut(&asset_id) {
                    asset.status = AssetStatus::Error(error.clone());
                    asset.touch();
                    me.event_bus
                        .emit(AppEvent::AssetUpdated { asset: asset.clone() });
                }
                me.event_bus.emit(AppEvent::JobFailed {
                    job_id,
                    error: error.clone(),
                });
                tracing::error!("Job {job_id} failed: {error}");
            }
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

                let asset = {
                    let store = self.assets.read().await;
                    store.get(&asset_id).cloned()
                };

                let Some(mut asset) = asset else {
                    self.job_repo
                        .set_failed(job_id, format!("Asset not found: {asset_id}"))
                        .await?;
                    self.event_bus.emit(AppEvent::JobFailed {
                        job_id,
                        error: "Asset not found".into(),
                    });
                    return Ok(());
                };

                asset.status = AssetStatus::Analyzing { progress: 5 };
                asset.touch();
                {
                    let mut store = self.assets.write().await;
                    store.insert(asset.id, asset.clone());
                }
                self.event_bus.emit(AppEvent::AssetUpdated {
                    asset: asset.clone(),
                });

                let media = self.media.clone();
                let path = asset.path.clone();
                let info = tokio::task::spawn_blocking(move || media.probe(&path))
                    .await
                    .map_err(|e| crate::AppError::Other(format!("Join error: {e}")))?
                    .map_err(|e| crate::AppError::Other(format!("Media probe failed: {e}")))?;

                asset.status = AssetStatus::Analyzing { progress: 80 };
                asset.touch();
                {
                    let mut store = self.assets.write().await;
                    store.insert(asset.id, asset.clone());
                }
                self.event_bus.emit(AppEvent::AssetUpdated {
                    asset: asset.clone(),
                });
                self.job_repo.set_progress(job_id, 80).await?;
                self.event_bus.emit(AppEvent::JobProgress {
                    job_id,
                    progress: 80,
                    message: Some("Finalizing…".into()),
                });

                asset.media_info = Some(info);
                asset.status = AssetStatus::Ready;
                asset.touch();
                {
                    let mut store = self.assets.write().await;
                    store.insert(asset.id, asset.clone());
                }

                self.event_bus
                    .emit(AppEvent::AssetAnalyzed { asset: asset.clone() });
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

                let asset = {
                    let store = self.assets.read().await;
                    store.get(&asset_id).cloned()
                };

                let Some(mut asset) = asset else {
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
                {
                    let mut store = self.assets.write().await;
                    store.insert(asset.id, asset.clone());
                }
                self.event_bus.emit(AppEvent::AssetUpdated {
                    asset: asset.clone(),
                });

                std::fs::create_dir_all(&self.proxy_dir).ok();

                if cancel.is_cancelled() {
                    self.job_repo.set_canceled(job_id).await?;
                    self.event_bus.emit(AppEvent::JobCanceled { job_id });

                    {
                        let mut store = self.assets.write().await;
                        if let Some(asset) = store.get_mut(&asset_id) {
                            asset.status = AssetStatus::Error("Proxy canceled".into());
                        }
                    }
                    return Ok(());
                }

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
                {
                    let mut store = self.assets.write().await;
                    store.insert(asset.id, asset.clone());
                }

                self.event_bus
                    .emit(AppEvent::AssetProxyComplete { asset: asset.clone() });
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
