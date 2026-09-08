//! Cross-platform file pickers (yadaw `file_picker.rs` pattern).
//!
//! [`Picker`] runs the rlobkit dialog off the UI thread and is polled each
//! frame from the UI pump; outcomes are [`CompletedPicker`] values applied on
//! the UI thread, so view code stays uniform across desktop, Android and web.
//!
//! - Desktop: real OS paths via the rlobkit async API (same UX as before).
//! - Android: SAF URIs are staged into app-private storage, so everything
//!   downstream keeps working with plain paths.
//! - Web: picked files arrive as bytes; project JSON is handled in-memory by
//!   the wasm backend, media bytes are registered without decoding.

use rlobkit_dialogs::picker::{OpenFileOptions, SaveFileOptions};
use rlobkit_dialogs::{PlatformFile, RlobKit, RlobKitMode, RlobKitType};
use snapshort_usecases::TimelineMarkerData;

pub const PROJECT_EXTENSIONS: &[&str] = &["snap"];
pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "m4a", "aac", "mp4", "mkv", "webm", "mov", "avi", "png", "jpg",
    "jpeg", "webp", "bmp",
];

pub struct Picker<T> {
    rx: Option<flume::Receiver<Result<Option<T>, String>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + 'static> Picker<T> {
    /// Spawn the picker off the UI thread. Native threads exist here, so the
    /// future must be Send.
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Option<T>, String>> + Send + 'static,
    {
        let (tx, rx) = flume::unbounded();
        web_workers::spawn_async_unified(move || async move {
            let result = f().await;
            let _ = tx.send(result);
        });
        Self { rx: Some(rx) }
    }
}

// Web file futures hold !Send JS handles, so this impl drops the Send bound
// (yadaw file_picker.rs does exactly the same split).
#[cfg(target_arch = "wasm32")]
impl<T: 'static> Picker<T> {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: FnOnce() -> Fut + 'static,
        Fut: std::future::Future<Output = Result<Option<T>, String>> + 'static,
    {
        let (tx, rx) = flume::unbounded();
        wasm_bindgen_futures::spawn_local(async move {
            let result = f().await;
            let _ = tx.send(result);
        });
        Self { rx: Some(rx) }
    }
}

impl<T> Picker<T> {
    /// An already-resolved picker (synthetic outcomes).
    pub fn ready(value: Option<T>) -> Self {
        let (tx, rx) = flume::unbounded();
        let _ = tx.send(Ok(value));
        Self { rx: Some(rx) }
    }
}

impl<T> Picker<T> {
    pub fn poll(&mut self) -> Option<Result<Option<T>, String>> {
        let rx = self.rx.as_mut()?;
        match rx.try_recv() {
            Ok(res) => {
                self.rx = None;
                Some(res)
            }
            Err(flume::TryRecvError::Empty) => None,
            Err(flume::TryRecvError::Disconnected) => {
                self.rx = None;
                Some(Err("Picker task disconnected".into()))
            }
        }
    }
}

/// A picker paired with what to do once it resolves. Polled in the UI pump.
pub enum ActivePicker {
    OpenProject(Picker<PlatformFile>),
    ImportMedia(Picker<Vec<PlatformFile>>),
    /// Relink one offline asset: the picked file's path becomes its source.
    RelinkAsset {
        picker: Picker<Vec<PlatformFile>>,
        asset_id: snapshort_usecases::AssetId,
    },
    /// Folder-wide relink: the picked file's parent dir is searched for
    /// every offline asset's filename (no directory picker on any platform).
    RelinkSearch(Picker<Vec<PlatformFile>>),
    SaveProject {
        picker: Picker<PlatformFile>,
        markers: Vec<TimelineMarkerData>,
    },
    /// Web direct download (no file dialog): serialize + download on apply.
    SaveDownload {
        markers: Vec<TimelineMarkerData>,
    },
    ExportPath(Picker<PlatformFile>),
}

/// A resolved picker, ready to apply on the UI thread.
pub enum CompletedPicker {
    OpenPath(std::path::PathBuf),
    OpenBytes { name: String, data: Vec<u8> },
    ImportPaths(Vec<std::path::PathBuf>),
    ImportBytes(Vec<(String, Vec<u8>)>),
    /// Single relink target (first picked file) for the pending asset.
    RelinkAssetPath {
        asset_id: snapshort_usecases::AssetId,
        path: std::path::PathBuf,
    },
    /// Parent dir of the picked file, to search for offline filenames.
    RelinkSearchDir(std::path::PathBuf),
    SavePath {
        path: std::path::PathBuf,
        markers: Vec<TimelineMarkerData>,
    },
    SaveDownload {
        name: Option<String>,
        markers: Vec<TimelineMarkerData>,
    },
    ExportPath(std::path::PathBuf),
    Cancelled,
    Failed(String),
}

fn custom_type(extensions: &[&str]) -> RlobKitType {
    RlobKitType::Custom {
        extensions: extensions.iter().map(|s| s.to_string()).collect(),
        mime_types: vec!["*/*".to_string()],
    }
}

/// Open a Snapshort project (file contents or staged path).
pub fn pick_open_project() -> Picker<PlatformFile> {
    Picker::new(move || async move {
        let result = RlobKit::open_file_picker(OpenFileOptions {
            file_type: custom_type(PROJECT_EXTENSIONS),
            mode: RlobKitMode::Single,
            title: Some("Open Project".to_string()),
            initial_directory: None,
        })
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.and_then(|mut files| files.pop()))
    })
}

/// Import media files.
pub fn pick_media_files() -> Picker<Vec<PlatformFile>> {
    Picker::new(move || async move {
        let result = RlobKit::open_file_picker(OpenFileOptions {
            file_type: custom_type(MEDIA_EXTENSIONS),
            mode: RlobKitMode::Multiple { limit: None },
            title: Some("Import Media".to_string()),
            initial_directory: None,
        })
        .await
        .map_err(|e| e.to_string())?;
        Ok(result)
    })
}

/// Choose a save destination. Desktop/Android resolve to a writable path
/// (Android stages into app-private projects/); web resolves to a download
/// target carrying the chosen file name.
pub fn pick_save_project(suggested_name: &str) -> Picker<PlatformFile> {
    let suggested = suggested_name.to_string();
    Picker::new(move || async move {
        let result = RlobKit::open_file_saver(SaveFileOptions {
            suggested_name: Some(suggested.clone()),
            extension: Some("snap".to_string()),
            title: Some("Save Project".to_string()),
            initial_directory: None,
            file_type: Some(custom_type(PROJECT_EXTENSIONS)),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
        Ok(result)
    })
}

/// Choose an export destination (desktop paths / Android app storage).
/// Web video export is unavailable; the export button short-circuits there.
pub fn pick_export_path() -> Picker<PlatformFile> {
    Picker::new(move || async move {
        let result = RlobKit::open_file_saver(SaveFileOptions {
            suggested_name: Some("export.mp4".to_string()),
            extension: Some("mp4".to_string()),
            title: Some("Export Video".to_string()),
            initial_directory: None,
            file_type: Some(custom_type(&["mp4"])),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
        Ok(result)
    })
}

/// Pick media files as a relink target (single) or a folder hint: the
/// first file's parent dir is searched for offline filenames.
pub fn pick_relink_file(title: &str) -> Picker<Vec<PlatformFile>> {
    let title = title.to_string();
    Picker::new(move || async move {
        let result = RlobKit::open_file_picker(OpenFileOptions {
            file_type: custom_type(MEDIA_EXTENSIONS),
            mode: RlobKitMode::Multiple { limit: None },
            title: Some(title.clone()),
            initial_directory: None,
        })
        .await
        .map_err(|e| e.to_string())?;
        Ok(result)
    })
}

fn platform_path(file: &PlatformFile) -> Option<std::path::PathBuf> {
    if let Some(path) = file.path() {
        return Some(path.to_path_buf());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Android SAF URIs: stage bytes so relink keeps plain paths.
        return stage_bytes(file);
    }
    #[cfg(target_arch = "wasm32")]
    {
        return None;
    }
}

fn first_path_as(
    asset_id: snapshort_usecases::AssetId,
    files: Vec<PlatformFile>,
) -> CompletedPicker {
    match files.first().and_then(platform_path) {
        Some(path) => CompletedPicker::RelinkAssetPath { asset_id, path },
        None => CompletedPicker::Failed("Could not read a path from the picked file".into()),
    }
}

fn parent_dir_as(files: Vec<PlatformFile>) -> CompletedPicker {
    match files
        .first()
        .and_then(platform_path)
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        Some(dir) => CompletedPicker::RelinkSearchDir(dir),
        None => CompletedPicker::Failed("Could not read a folder from the picked file".into()),
    }
}

/// Save without a file dialog (wasm direct download path). The file name
/// resolves at apply time from the project.
pub fn ready_save_download(markers: Vec<TimelineMarkerData>) -> ActivePicker {
    ActivePicker::SaveDownload { markers }
}

/// Install an in-flight picker (replaces any previous one).
pub fn start_picker(store: &crate::state::Store, picker: ActivePicker) {
    *store.state.active_picker.borrow_mut() = Some(picker);
}

/// Poll the slot; returns a completed outcome once resolved (slot cleared).
pub fn drain_picker(store: &crate::state::Store) -> Option<CompletedPicker> {
    let mut slot = store.state.active_picker.borrow_mut();
    let picker = slot.as_mut()?;
    let outcome = poll_active(picker)?;
    slot.take();
    Some(outcome)
}

/// Poll the active picker; returns a completed outcome once resolved.
pub fn poll_active(picker: &mut ActivePicker) -> Option<CompletedPicker> {
    match picker {
        ActivePicker::OpenProject(p) => {
            let file = p.poll()?;
            Some(match file {
                Ok(Some(f)) => file_to_open(f),
                Ok(None) => CompletedPicker::Cancelled,
                Err(e) => CompletedPicker::Failed(e),
            })
        }
        ActivePicker::ImportMedia(p) => {
            let files = p.poll()?;
            Some(match files {
                Ok(Some(fs)) => files_to_import(fs),
                Ok(None) => CompletedPicker::Cancelled,
                Err(e) => CompletedPicker::Failed(e),
            })
        }
        ActivePicker::RelinkAsset { picker, asset_id } => {
            let files = picker.poll()?;
            Some(match files {
                Ok(Some(fs)) => first_path_as(*asset_id, fs),
                Ok(None) => CompletedPicker::Cancelled,
                Err(e) => CompletedPicker::Failed(e),
            })
        }
        ActivePicker::RelinkSearch(p) => {
            let files = p.poll()?;
            Some(match files {
                Ok(Some(fs)) => parent_dir_as(fs),
                Ok(None) => CompletedPicker::Cancelled,
                Err(e) => CompletedPicker::Failed(e),
            })
        }
        ActivePicker::SaveProject { picker, markers } => {
            let file = picker.poll()?;
            Some(match file {
                Ok(Some(f)) => file_to_save(f, std::mem::take(markers)),
                Ok(None) => {
                    let _ = std::mem::take(markers);
                    CompletedPicker::Cancelled
                }
                Err(e) => CompletedPicker::Failed(e),
            })
        }
        ActivePicker::SaveDownload { markers } => {
            return Some(CompletedPicker::SaveDownload {
                name: None,
                markers: std::mem::take(markers),
            });
        }
        ActivePicker::ExportPath(p) => {
            let file = p.poll()?;
            Some(match file {
                Ok(Some(f)) => file_to_export(f),
                Ok(None) => CompletedPicker::Cancelled,
                Err(e) => CompletedPicker::Failed(e),
            })
        }
    }
}

fn file_bytes(file: &PlatformFile) -> Option<Vec<u8>> {
    file.data()
        .map(|b| b.to_vec())
        .or_else(|| file.read_bytes().ok().map(|b| b.to_vec()))
}

fn file_to_open(file: PlatformFile) -> CompletedPicker {
    if let Some(path) = file.path() {
        return CompletedPicker::OpenPath(path.to_path_buf());
    }
    match file_bytes(&file) {
        Some(data) => CompletedPicker::OpenBytes {
            name: file.name().to_string(),
            data,
        },
        None => CompletedPicker::Failed(format!("Could not read {}", file.name())),
    }
}

fn files_to_import(files: Vec<PlatformFile>) -> CompletedPicker {
    #[cfg(target_arch = "wasm32")]
    {
        let mut out = Vec::new();
        for f in &files {
            match file_bytes(f) {
                Some(data) => out.push((f.name().to_string(), data)),
                None => return CompletedPicker::Failed(format!("Could not read {}", f.name())),
            }
        }
        return CompletedPicker::ImportBytes(out);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut paths = Vec::new();
        for f in &files {
            if let Some(path) = f.path() {
                paths.push(path.to_path_buf());
                continue;
            }
            // Android SAF URIs: stage bytes into app-private imports/.
            match stage_bytes(f) {
                Some(p) => paths.push(p),
                None => return CompletedPicker::Failed(format!("Could not read {}", f.name())),
            }
        }
        return CompletedPicker::ImportPaths(paths);
    }
}

fn file_to_save(file: PlatformFile, markers: Vec<TimelineMarkerData>) -> CompletedPicker {
    if let Some(path) = file.path() {
        return CompletedPicker::SavePath {
            path: path.to_path_buf(),
            markers,
        };
    }
    #[cfg(target_arch = "wasm32")]
    {
        return CompletedPicker::SaveDownload {
            name: Some(file.name().to_string()),
            markers,
        };
    }
    // Android SAF saver: persist under the chosen name in app storage.
    #[cfg(target_os = "android")]
    {
        let name = if file.name().is_empty() {
            "project.snap".to_string()
        } else {
            file.name().to_string()
        };
        return CompletedPicker::SavePath {
            path: android_save_path(&name),
            markers,
        };
    }
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    {
        let _ = markers;
        return CompletedPicker::Failed(format!(
            "Saver returned no path for {}",
            file.name()
        ));
    }
}

fn file_to_export(file: PlatformFile) -> CompletedPicker {
    if let Some(path) = file.path() {
        return CompletedPicker::ExportPath(path.to_path_buf());
    }
    #[cfg(target_os = "android")]
    {
        return CompletedPicker::ExportPath(android_save_path(file.name()));
    }
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    {
        return CompletedPicker::Failed(format!(
            "Saver returned no path for {}",
            file.name()
        ));
    }
    #[cfg(target_arch = "wasm32")]
    {
        return CompletedPicker::Failed("Video export is not available on web yet.".into());
    }
}

/// Stage picked bytes into app-private storage (Android SAF URIs).
#[cfg(not(target_arch = "wasm32"))]
fn stage_bytes(file: &PlatformFile) -> Option<std::path::PathBuf> {
    let data = file_bytes(file)?;
    let dir = staging_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut name = file.name().to_string();
    if name.is_empty() {
        name = format!("import-{}", web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0));
    }
    let dest = dir.join(name);
    std::fs::write(&dest, data).ok()?;
    Some(dest)
}

#[cfg(target_os = "android")]
fn staging_dir() -> std::path::PathBuf {
    let base = game_utils::android_data_dir("org.mlm.snapshort");
    base.join("imports")
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
fn staging_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("snapshort-imports")
}

#[cfg(target_os = "android")]
fn android_save_path(name: &str) -> std::path::PathBuf {
    let base = game_utils::android_data_dir("org.mlm.snapshort");
    let dir = base.join("projects");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(if name.is_empty() { "project.snap" } else { name })
}
