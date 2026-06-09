use std::path::PathBuf;

pub type Frame = i64;

#[derive(Debug, Clone, Copy)]
pub struct FrameRange {
    pub start: Frame,
    pub end: Frame,
}

impl FrameRange {
    pub fn new(start: Frame, end: Frame) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone)]
pub struct SceneChange {
    pub frame: Frame,
    pub confidence: f64,
    pub change_type: SceneChangeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneChangeType {
    HardCut,
    Fade,
    Dissolve,
    Wipe,
    ContentChange,
}

#[derive(Debug, Clone)]
pub struct EditSuggestion {
    pub id: uuid::Uuid,
    pub suggestion_type: SuggestionType,
    pub frame_range: FrameRange,
    pub confidence: f64,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionType {
    RemoveSilence,
    TrimRedundant,
    CutPoint,
    AddTransition,
    SpeedUp,
    SlowDown,
    RemoveFillerWords,
    NormalizeAudio,
}

#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    pub start: Frame,
    pub end: Frame,
    pub text: String,
    pub confidence: f64,
    pub speaker_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub segments: Vec<TranscriptSegment>,
    pub language: Option<String>,
    pub average_confidence: f64,
}

impl Transcript {
    pub fn full_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn search(&self, term: &str) -> Vec<&TranscriptSegment> {
        let term_lower = term.to_lowercase();
        self.segments
            .iter()
            .filter(|s| s.text.to_lowercase().contains(&term_lower))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ContentAnalysis {
    pub faces: Vec<FaceDetection>,
    pub objects: Vec<ObjectDetection>,
    pub categories: Vec<ContentCategory>,
    pub audio_analysis: Option<AudioAnalysis>,
}

#[derive(Debug, Clone)]
pub struct FaceDetection {
    pub frame_range: FrameRange,
    pub bounding_box: (f32, f32, f32, f32),
    pub confidence: f64,
    pub face_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectDetection {
    pub frame: Frame,
    pub label: String,
    pub bounding_box: (f32, f32, f32, f32),
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct ContentCategory {
    pub name: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct AudioAnalysis {
    pub speech_segments: Vec<FrameRange>,
    pub music_segments: Vec<FrameRange>,
    pub silence_segments: Vec<FrameRange>,
    pub loudness_lufs: f64,
    pub peak_db: f64,
}

#[derive(Debug, Clone)]
pub enum AiError {
    ModelNotAvailable(String),
    ProcessingFailed(String),
    UnsupportedMedia(String),
    ServiceUnavailable(String),
    RateLimited,
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelNotAvailable(model) => write!(f, "Model not available: {}", model),
            Self::ProcessingFailed(msg) => write!(f, "Processing failed: {}", msg),
            Self::UnsupportedMedia(msg) => write!(f, "Unsupported media: {}", msg),
            Self::ServiceUnavailable(msg) => write!(f, "Service unavailable: {}", msg),
            Self::RateLimited => write!(f, "Rate limited"),
        }
    }
}

impl std::error::Error for AiError {}

pub struct AiService {
    local_models_available: bool,
}

impl Default for AiService {
    fn default() -> Self {
        Self::new()
    }
}

impl AiService {
    pub fn new() -> Self {
        Self {
            local_models_available: false,
        }
    }

    pub fn is_scene_detection_available(&self) -> bool {
        true
    }

    pub fn is_transcription_available(&self) -> bool {
        true
    }

    pub fn is_auto_edit_available(&self) -> bool {
        true
    }

    pub fn detect_scenes(&self, _asset_path: &PathBuf) -> Result<Vec<SceneChange>, AiError> {
        Ok(vec![])
    }

    pub fn transcribe(&self, _asset_path: &PathBuf) -> Result<Transcript, AiError> {
        Ok(Transcript {
            segments: vec![],
            language: None,
            average_confidence: 0.0,
        })
    }

    pub fn suggest_edits(&self, _timeline: &miniter_domain::Timeline) -> Result<Vec<EditSuggestion>, AiError> {
        Ok(vec![])
    }

    pub fn analyze_content(&self, _asset_path: &PathBuf) -> Result<ContentAnalysis, AiError> {
        Ok(ContentAnalysis {
            faces: vec![],
            objects: vec![],
            categories: vec![],
            audio_analysis: None,
        })
    }

    pub fn detect_silence(
        &self,
        _asset_path: &PathBuf,
        _threshold_db: f64,
        _min_duration_frames: i64,
    ) -> Result<Vec<FrameRange>, AiError> {
        Ok(vec![])
    }

    pub fn has_local_models(&self) -> bool {
        self.local_models_available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_service_creation() {
        let service = AiService::new();
        assert!(service.is_scene_detection_available());
        assert!(service.is_transcription_available());
    }

    #[test]
    fn test_transcript_full_text() {
        let transcript = Transcript {
            segments: vec![
                TranscriptSegment {
                    start: 0,
                    end: 24,
                    text: "Hello".into(),
                    confidence: 0.9,
                    speaker_id: None,
                },
                TranscriptSegment {
                    start: 24,
                    end: 48,
                    text: "world".into(),
                    confidence: 0.95,
                    speaker_id: None,
                },
            ],
            language: Some("en".into()),
            average_confidence: 0.925,
        };

        assert_eq!(transcript.full_text(), "Hello world");
    }

    #[test]
    fn test_transcript_search() {
        let transcript = Transcript {
            segments: vec![
                TranscriptSegment {
                    start: 0,
                    end: 24,
                    text: "The quick brown fox".into(),
                    confidence: 0.9,
                    speaker_id: None,
                },
                TranscriptSegment {
                    start: 24,
                    end: 48,
                    text: "jumps over".into(),
                    confidence: 0.95,
                    speaker_id: None,
                },
            ],
            language: Some("en".into()),
            average_confidence: 0.925,
        };

        let results = transcript.search("fox");
        assert_eq!(results.len(), 1);
        assert!(results[0].text.contains("fox"));
    }
}
