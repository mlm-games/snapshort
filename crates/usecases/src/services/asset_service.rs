use crate::jobs_service::{JobSpec, JobsService};
use crate::{
    AppError, AppEvent, AppResult, Asset, AssetCommand, AssetId, AssetStatus, AssetType, EventBus,
    ProxyPolicy, should_auto_proxy,
};
use std::collections::HashMap;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::{instrument, warn};

pub struct AssetService {
    event_bus: EventBus,
    assets: Arc<RwLock<HashMap<AssetId, Asset>>>,
    jobs: Arc<JobsService>,
    policy: Arc<RwLock<ProxyPolicy>>,
}

impl AssetService {
    pub fn new(event_bus: EventBus, jobs: Arc<JobsService>) -> Self {
        Self {
            event_bus,
            assets: Arc::new(RwLock::new(HashMap::new())),
            jobs,
            policy: Arc::new(RwLock::new(ProxyPolicy::default())),
        }
    }

    pub async fn load_assets(&self, assets: Vec<Asset>) {
        let mut store = self.assets.write().await;
        store.clear();
        for asset in assets {
            store.insert(asset.id, asset);
        }
    }

    pub async fn list(&self) -> Vec<Asset> {
        self.assets.read().await.values().cloned().collect()
    }

    pub async fn get(&self, id: AssetId) -> Option<Asset> {
        self.assets.read().await.get(&id).cloned()
    }

    pub async fn asset_paths(&self) -> HashMap<AssetId, PathBuf> {
        self.assets
            .read()
            .await
            .iter()
            .map(|(id, asset)| (*id, asset.effective_path().clone()))
            .collect()
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: AssetCommand) -> AppResult<()> {
        match command {
            AssetCommand::Import { paths } => {
                self.import_files(paths).await?;
            }
            AssetCommand::Analyze { asset_id } => {
                let _ = self.jobs.submit(JobSpec::AnalyzeAsset { asset_id }).await?;
            }
            AssetCommand::GenerateProxy { asset_id } => {
                let _ = self
                    .jobs
                    .submit(JobSpec::GenerateProxy { asset_id })
                    .await?;
            }
            AssetCommand::Delete { asset_id } => {
                self.delete_asset(asset_id).await?;
            }
            AssetCommand::UpdateMetadata {
                asset_id,
                name,
                tags,
                rating,
            } => {
                self.update_metadata(asset_id, name, tags, rating).await?;
            }
            AssetCommand::SetProxyPolicy {
                auto_generate,
                min_width,
            } => {
                *self.policy.write().await = ProxyPolicy {
                    auto_generate,
                    min_width: min_width.max(1),
                };
            }
            AssetCommand::Relink { asset_id, new_path } => {
                self.relink_asset(asset_id, &new_path).await?;
            }
            AssetCommand::RelinkInFolder { dir } => {
                self.relink_in_folder(&dir).await?;
            }
        }
        Ok(())
    }

    /// Sync an analyzed asset from the media pipeline into this map and apply
    /// the proxy policy: qualifying video auto-submits a proxy job. Called
    /// from the backend event forwarder (the job's own map is separate).
    pub async fn note_analyzed(&self, asset: Asset) {
        self.assets.write().await.insert(asset.id, asset.clone());
        if should_auto_proxy(&*self.policy.read().await, &asset) {
            if let Err(e) = self
                .jobs
                .submit(JobSpec::GenerateProxy { asset_id: asset.id })
                .await
            {
                warn!("Auto-proxy submit failed for {}: {e}", asset.id);
            }
        }
    }

    /// Mark every in-memory asset whose file is gone as Offline.
    ///
    /// The backend runs this right after loading a project's assets, so
    /// files that vanished between sessions show honest state instead of
    /// failing later as opaque job errors. Returns the updated list for the
    /// caller to fan out into the other services. Web has no filesystem, so
    /// this is a pass-through there (everything would read as missing).
    pub async fn mark_missing_offline(&self) -> Vec<Asset> {
        #[cfg(target_arch = "wasm32")]
        {
            return self.list().await;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut changed = Vec::new();
            {
                let mut store = self.assets.write().await;
                for asset in store.values_mut() {
                    if !matches!(asset.status, AssetStatus::Offline) && !asset.path.exists() {
                        asset.status = AssetStatus::Offline;
                        asset.touch();
                        changed.push(asset.clone());
                    }
                }
            }
            for asset in &changed {
                self.event_bus.emit(AppEvent::AssetUpdated {
                    asset: asset.clone(),
                });
            }
            if !changed.is_empty() {
                let names: Vec<&str> = changed.iter().take(5).map(|a| a.name.as_str()).collect();
                warn!(
                    "Marked {} asset(s) offline (files vanished): {}…",
                    changed.len(),
                    names.join(", ")
                );
            }
            self.list().await
        }
    }

    /// Relink one offline asset at a new path, then auto-relink any other
    /// offline assets whose filenames exist next to it. Returns all relinked
    /// ids (requested first) and emits exactly one summary event, so the
    /// requested relink always toasts — even with no siblings around.
    pub async fn relink_asset(
        &self,
        asset_id: AssetId,
        new_path: &Path,
    ) -> AppResult<Vec<AssetId>> {
        if !new_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Relink target does not exist: {}",
                new_path.display()
            )));
        }
        if self.get(asset_id).await.is_none() {
            return Err(AppError::AssetNotFound(asset_id.0));
        }
        let mut relinked = vec![self.apply_relink(asset_id, new_path).await?];
        // Resolve-style "relink others": siblings by filename in the new dir.
        let dir = new_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| new_path.to_path_buf());
        relinked.extend(self.scan_folder(&dir).await);
        self.event_bus.emit(AppEvent::AssetsRelinked {
            relinked: relinked.clone(),
            dir,
        });
        Ok(relinked)
    }

    /// Relink every offline asset whose filename exists under `dir`.
    /// Always emits the summary event — even empty, so a folder search with
    /// no matches answers instead of going quiet. Per-asset rows arrive via
    /// `AssetUpdated`.
    pub async fn relink_in_folder(&self, dir: &Path) -> AppResult<Vec<AssetId>> {
        if !dir.is_dir() {
            return Err(AppError::InvalidInput(format!(
                "Relink folder is not a directory: {}",
                dir.display()
            )));
        }
        let relinked = self.scan_folder(dir).await;
        self.event_bus.emit(AppEvent::AssetsRelinked {
            relinked: relinked.clone(),
            dir: dir.to_path_buf(),
        });
        Ok(relinked)
    }

    /// Filename-match offline assets against `dir` without emitting.
    async fn scan_folder(&self, dir: &Path) -> Vec<AssetId> {
        let offline: Vec<(AssetId, String)> = {
            let store = self.assets.read().await;
            store
                .values()
                .filter(|a| matches!(a.status, AssetStatus::Offline))
                .filter_map(|a| {
                    a.path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| (a.id, n.to_string()))
                })
                .collect()
        };
        let mut relinked = Vec::new();
        for (id, name) in offline {
            let candidate = dir.join(&name);
            if candidate.exists() {
                // A concurrent delete between the snapshot above and here
                // just skips that asset instead of failing the whole folder.
                if self.apply_relink(id, &candidate).await.is_ok() {
                    relinked.push(id);
                }
            }
        }
        relinked
    }

    /// Point an asset at a verified-existing path: drop the stale proxy
    /// (it was rendered from different media), clear analysis, and requeue.
    /// Emits `AssetUpdated`; callers emit the summary event.
    async fn apply_relink(&self, asset_id: AssetId, new_path: &Path) -> AppResult<AssetId> {
        let asset = {
            let mut store = self.assets.write().await;
            let Some(asset) = store.get_mut(&asset_id) else {
                return Err(AppError::AssetNotFound(asset_id.0));
            };
            if let Some(proxy) = asset.proxy.take() {
                if let Err(e) = std::fs::remove_file(&proxy.path) {
                    tracing::debug!(
                        "Could not delete stale proxy file {}: {e}",
                        proxy.path.display()
                    );
                }
            }
            // Refresh the display name when it still mirrors the old file
            // stem; a user rename (UpdateMetadata) is left alone.
            let old_stem = asset
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if asset.name == old_stem {
                asset.name = new_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
            }
            asset.path = new_path.to_path_buf();
            asset.media_info = None;
            asset.status = AssetStatus::Analyzing { progress: 0 };
            asset.touch();
            asset.clone()
        };
        self.event_bus.emit(AppEvent::AssetUpdated {
            asset: asset.clone(),
        });
        // Sync into the job map (the analyze job reads it) and requeue.
        self.jobs.insert_asset(asset.clone()).await;
        let _ = self
            .jobs
            .submit(JobSpec::AnalyzeAsset { asset_id })
            .await?;
        Ok(asset_id)
    }

    #[instrument(skip(self))]
    async fn import_files(&self, paths: Vec<PathBuf>) -> AppResult<Vec<Asset>> {
        let mut assets = Vec::new();
        for path in paths {
            let asset_type = detect_asset_type(&path);
            let mut asset = Asset::new(path.clone(), asset_type);

            if !path.exists() {
                // Keep the row so timeline references stay valid and the
                // missing file shows an honest Offline state with a Relink
                // action instead of vanishing silently.
                warn!("File not found at import: {}", path.display());
                asset.status = AssetStatus::Offline;
                let mut store = self.assets.write().await;
                store.insert(asset.id, asset.clone());
                self.event_bus.emit(AppEvent::AssetImported {
                    asset: asset.clone(),
                });
                assets.push(asset);
                continue;
            }

            asset.status = AssetStatus::Analyzing { progress: 0 };

            let mut store = self.assets.write().await;
            store.insert(asset.id, asset.clone());
            self.event_bus.emit(AppEvent::AssetImported {
                asset: asset.clone(),
            });
            assets.push(asset.clone());

            // Sync into JobsService's asset map so the analyze job can read it
            self.jobs.insert_asset(asset.clone()).await;

            let _ = self
                .jobs
                .submit(JobSpec::AnalyzeAsset { asset_id: asset.id })
                .await?;
        }

        Ok(assets)
    }

    #[instrument(skip(self))]
    async fn delete_asset(&self, asset_id: AssetId) -> AppResult<()> {
        let mut store = self.assets.write().await;
        if let Some(asset) = store.remove(&asset_id) {
            if let Some(proxy) = asset.proxy {
                // Best-effort: the row is gone regardless; a leftover file
                // is an orphan, not a correctness issue.
                if let Err(e) = std::fs::remove_file(&proxy.path) {
                    tracing::debug!("Could not delete proxy file {}: {e}", proxy.path.display());
                }
            }
            self.event_bus.emit(AppEvent::AssetDeleted { asset_id });
        }
        Ok(())
    }

    #[instrument(skip(self))]
    async fn update_metadata(
        &self,
        asset_id: AssetId,
        name: Option<String>,
        tags: Option<Vec<String>>,
        rating: Option<u8>,
    ) -> AppResult<()> {
        let mut store = self.assets.write().await;
        let Some(asset) = store.get_mut(&asset_id) else {
            return Err(AppError::AssetNotFound(asset_id.0));
        };

        if let Some(name) = name {
            asset.name = name;
        }
        if let Some(tags) = tags {
            asset.tags = tags;
        }
        if let Some(r) = rating {
            asset.rating = Some(r.min(5));
        }

        asset.touch();
        self.event_bus.emit(AppEvent::AssetUpdated {
            asset: asset.clone(),
        });

        Ok(())
    }
}

fn detect_asset_type(path: &PathBuf) -> AssetType {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mov" | "mkv" | "webm" | "avi" => AssetType::Video,
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" => AssetType::Audio,
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "tiff" => AssetType::Image,
        other => {
            tracing::warn!(
                "Unknown file extension '.{other}' for '{}', treating as Video",
                path.display()
            );
            AssetType::Video
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snapshort_infra_media::{MediaInfo, VideoStream};
    use snapshort_infra_store::JobStore;
    use std::path::Path;

    fn analyzed_video(width: u32) -> Asset {
        let mut asset = Asset::new(
            std::path::PathBuf::from("/tmp/does-not-exist.mp4"),
            AssetType::Video,
        );
        asset.status = AssetStatus::Ready;
        asset.media_info = Some(MediaInfo {
            container: "mp4".into(),
            duration_ms: 10_000,
            file_size: 50_000_000,
            video_streams: vec![VideoStream {
                codec_name: "h264".into(),
                codec_profile: "high".into(),
                bit_depth: Some(8),
                chroma_subsampling: Some("4:2:0".into()),
                width,
                height: 2160,
                fps: 30.0,
                duration_frames: 300,
                pixel_format: "yuv420p".into(),
                color_space: "bt709".into(),
                hdr: false,
            }],
            audio_streams: vec![],
            waveform: None,
        });
        asset
    }

    fn service_in(dir: &Path) -> (AssetService, JobStore) {
        let store = JobStore::new(dir.join("jobs"));
        let jobs = Arc::new(JobsService::new(
            store.clone(),
            EventBus::new(),
            dir.join("proxies"),
        ));
        (AssetService::new(EventBus::new(), jobs), store)
    }

    #[tokio::test]
    async fn analyzed_wide_video_upserts_and_auto_submits_proxy() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        let asset = analyzed_video(3840);
        let id = asset.id;
        svc.note_analyzed(asset).await;

        // Stored for snapshots/preview…
        assert!(svc.get(id).await.is_some());
        // …and a proxy job is queued (default policy: on, ≥1920px).
        assert_eq!(store.list_pending().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn narrow_video_upserts_without_proxy_job() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        let asset = analyzed_video(1280);
        let id = asset.id;
        svc.note_analyzed(asset).await;
        assert!(svc.get(id).await.is_some());
        assert!(store.list_pending().unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabled_policy_upserts_without_proxy_job() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        svc.execute(AssetCommand::SetProxyPolicy {
            auto_generate: false,
            min_width: 1,
        })
        .await
        .unwrap();
        svc.note_analyzed(analyzed_video(7680)).await;
        assert!(store.list_pending().unwrap().is_empty());
    }
}

#[cfg(test)]
mod relink_tests {
    use super::*;
    use snapshort_infra_store::JobStore;
    use std::io::Write;

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"fake media").unwrap();
        path
    }

    fn offline_asset(missing: PathBuf) -> Asset {
        let mut asset = Asset::new(missing, AssetType::Video);
        asset.status = AssetStatus::Offline;
        asset
    }

    fn service_in(dir: &Path) -> (AssetService, JobStore) {
        let store = JobStore::new(dir.join("jobs"));
        let jobs = Arc::new(JobsService::new(
            store.clone(),
            EventBus::new(),
            dir.join("proxies"),
        ));
        (AssetService::new(EventBus::new(), jobs), store)
    }

    #[tokio::test]
    async fn import_missing_file_creates_offline_row() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _) = service_in(dir.path());
        let missing = dir.path().join("gone.mp4");
        svc.execute(AssetCommand::Import {
            paths: vec![missing.clone()],
        })
        .await
        .unwrap();
        let list = svc.list().await;
        assert_eq!(list.len(), 1);
        assert!(matches!(list[0].status, AssetStatus::Offline));
        assert_eq!(list[0].path, missing);
    }

    #[tokio::test]
    async fn mark_missing_offline_flags_only_vanished_files() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _) = service_in(dir.path());
        let present = write_file(dir.path(), "here.mp4");
        let mut a = Asset::new(present, AssetType::Video);
        a.status = AssetStatus::Ready;
        let b = offline_asset(dir.path().join("already-offline.mp4"));
        let c = offline_asset(dir.path().join("gone.mp4"));
        svc.load_assets(vec![a.clone(), b, c]).await;

        let updated = svc.mark_missing_offline().await;
        let by_id: HashMap<AssetId, Asset> =
            updated.into_iter().map(|a| (a.id, a)).collect();
        // Present file untouched…
        assert!(matches!(by_id[&a.id].status, AssetStatus::Ready));
        // …vanished one flagged.
        assert!(by_id.values().any(
            |a| a.path.ends_with("gone.mp4")
                && matches!(a.status, AssetStatus::Offline)
        ));
    }

    #[tokio::test]
    async fn relink_moves_offline_to_analyzing_and_requeues() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        let target = write_file(dir.path(), "found.mp4");
        let asset = offline_asset(dir.path().join("old.mp4"));
        let id = asset.id;
        svc.load_assets(vec![asset]).await;

        let relinked = svc.relink_asset(id, &target).await.unwrap();
        assert_eq!(relinked, vec![id]);

        let back = svc.get(id).await.unwrap();
        assert_eq!(back.path, target);
        assert!(matches!(
            back.status,
            AssetStatus::Analyzing { .. }
        ));
        // Name tracked the old stem, so it follows the new file…
        assert_eq!(back.name, "found");
        // …and the analyze job is queued (probe will fail offline on fake
        // bytes, but the row + job plumbing is what this tests).
        assert_eq!(store.list_pending().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn relink_rejects_missing_target_and_keeps_state() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        let asset = offline_asset(dir.path().join("old.mp4"));
        let id = asset.id;
        svc.load_assets(vec![asset]).await;

        let err = svc
            .relink_asset(id, &dir.path().join("nope.mp4"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        assert!(matches!(svc.get(id).await.unwrap().status, AssetStatus::Offline));
        assert!(store.list_pending().unwrap().is_empty());
    }

    #[tokio::test]
    async fn relink_preserves_user_rename_and_drops_stale_proxy() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _) = service_in(dir.path());
        let target = write_file(dir.path(), "found.mp4");
        let proxy_file = write_file(dir.path(), "stale-proxy.mp4");
        let mut asset = offline_asset(dir.path().join("old.mp4"));
        asset.name = "My Custom Name".into();
        asset.proxy = Some(snapshort_infra_media::ProxyInfo {
            path: proxy_file.clone(),
            codec: "h264".into(),
            bitrate_kbps: 2000,
            fps: 30.0,
            width: 960,
            height: 540,
            created_at: chrono::Utc::now(),
        });
        let id = asset.id;
        svc.load_assets(vec![asset]).await;

        svc.relink_asset(id, &target).await.unwrap();
        let back = svc.get(id).await.unwrap();
        assert_eq!(back.name, "My Custom Name");
        assert!(back.proxy.is_none());
        assert!(!proxy_file.exists());
    }

    #[tokio::test]
    async fn folder_relink_matches_by_filename_and_skips_others() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _) = service_in(dir.path());
        let media = dir.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        write_file(&media, "a.mp4");
        let a = offline_asset(PathBuf::from("/old/a.mp4"));
        let b = offline_asset(PathBuf::from("/old/b.mp4"));
        svc.load_assets(vec![a.clone(), b.clone()]).await;

        let relinked = svc.relink_in_folder(&media).await.unwrap();
        assert_eq!(relinked, vec![a.id]);
        assert_eq!(
            svc.get(a.id).await.unwrap().path,
            media.join("a.mp4")
        );
        assert!(matches!(
            svc.get(b.id).await.unwrap().status,
            AssetStatus::Offline
        ));
    }

    #[tokio::test]
    async fn folder_relink_rejects_non_directories() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _) = service_in(dir.path());
        let err = svc
            .relink_in_folder(&dir.path().join("nope"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn relink_auto_matches_siblings_in_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _) = service_in(dir.path());
        let media = dir.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        write_file(&media, "a.mp4");
        write_file(&media, "b.mp4");
        let a = offline_asset(PathBuf::from("/old/a.mp4"));
        let b = offline_asset(PathBuf::from("/old/b.mp4"));
        svc.load_assets(vec![a.clone(), b.clone()]).await;

        // Manual relink of `a` pulls sibling `b` along (Resolve behavior).
        let relinked = svc.relink_asset(a.id, &media.join("a.mp4")).await.unwrap();
        assert_eq!(relinked.len(), 2);
        assert!(relinked.contains(&a.id) && relinked.contains(&b.id));
        assert!(matches!(
            svc.get(b.id).await.unwrap().status,
            AssetStatus::Analyzing { .. }
        ));
    }
}

#[cfg(test)]
mod relink_event_tests {
    use super::*;
    use snapshort_infra_store::JobStore;
    use web_time::Duration;

    fn service_with_bus(
        dir: &Path,
    ) -> (
        AssetService,
        JobStore,
        flume::Receiver<AppEvent>,
    ) {
        let bus = EventBus::new();
        let rx = bus.receiver();
        let store = JobStore::new(dir.join("jobs"));
        let jobs = Arc::new(JobsService::new(
            store.clone(),
            EventBus::new(),
            dir.join("proxies"),
        ));
        (
            AssetService::new(bus, jobs),
            store,
            rx,
        )
    }

    async fn next_relink_summary(rx: &flume::Receiver<AppEvent>) -> Vec<AssetId> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for relink summary"
            );
            let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv_async())
                .await
                .expect("timed out waiting for relink event")
                .expect("event channel closed");
            match ev {
                AppEvent::AssetsRelinked { relinked, .. } => return relinked,
                // apply_relink emits per-asset rows first; skip past them.
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn folder_search_with_no_matches_still_answers() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _, rx) = service_with_bus(dir.path());
        let mut asset = Asset::new(dir.path().join("gone.mp4").into(), AssetType::Video);
        asset.status = AssetStatus::Offline;
        svc.load_assets(vec![asset]).await;

        let relinked = svc.relink_in_folder(dir.path()).await.unwrap();
        assert!(relinked.is_empty());
        // …but the UI still gets its summary instead of silence.
        assert!(next_relink_summary(&rx).await.is_empty());
    }

    #[tokio::test]
    async fn single_relink_reports_requested_id_without_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, _, rx) = service_with_bus(dir.path());
        let target = dir.path().join("found.mp4");
        std::fs::write(&target, b"fake media").unwrap();
        let mut asset = Asset::new(dir.path().join("old.mp4").into(), AssetType::Video);
        asset.status = AssetStatus::Offline;
        let id = asset.id;
        svc.load_assets(vec![asset]).await;

        let relinked = svc.relink_asset(id, &target).await.unwrap();
        assert_eq!(relinked, vec![id]);
        assert_eq!(next_relink_summary(&rx).await, vec![id]);
    }
}
