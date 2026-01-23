//! AI-powered features infrastructure
//!
//! This crate provides AI-powered features for the Snapshort video editor,
//! including auto-editing, scene detection, and content analysis.

use snapshort_domain::prelude::*;
use std::path::PathBuf;

/// Scene detection result
#[derive(Debug, Clone)]
pub struct SceneChange {
    /// Frame where the scene change occurs
    pub frame: Frame,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Type of scene change detected
    pub change_type: SceneChangeType,
}

/// Type of scene change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneChangeType {
    /// Hard cut between scenes
    HardCut,
    /// Fade transition
    Fade,
    /// Dissolve transition
    Dissolve,
    /// Wipe transition
    Wipe,
    /// Content-based scene change (same shot, different content)
    ContentChange,
}

/// Auto-edit suggestion
#[derive(Debug, Clone)]
pub struct EditSuggestion {
    /// Unique suggestion ID
    pub id: uuid::Uuid,
    /// Type of edit suggested
    pub suggestion_type: SuggestionType,
    /// Frame range affected by this suggestion
    pub frame_range: FrameRange,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Human-readable description
    pub description: String,
}

/// Type of edit suggestion
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionType {
    /// Remove dead air / silence
    RemoveSilence,
    /// Trim redundant footage
    TrimRedundant,
    /// Suggested cut point
    CutPoint,
    /// Add transition
    AddTransition,
    /// Speed up segment
    SpeedUp,
    /// Slow down segment
    SlowDown,
    /// Remove filler words
    RemoveFillerWords,
    /// Improve audio levels
    NormalizeAudio,
}

/// Transcript segment from speech-to-text
#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    /// Start frame
    pub start: Frame,
    /// End frame
    pub end: Frame,
    /// Transcribed text
    pub text: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Speaker ID (if speaker diarization is enabled)
    pub speaker_id: Option<String>,
}

/// Full transcript result
#[derive(Debug, Clone)]
pub struct Transcript {
    /// All transcript segments
    pub segments: Vec<TranscriptSegment>,
    /// Detected language
    pub language: Option<String>,
    /// Average confidence across all segments
    pub average_confidence: f64,
}

impl Transcript {
    /// Get full text as a single string
    pub fn full_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Find segments containing a search term
    pub fn search(&self, term: &str) -> Vec<&TranscriptSegment> {
        let term_lower = term.to_lowercase();
        self.segments
            .iter()
            .filter(|s| s.text.to_lowercase().contains(&term_lower))
            .collect()
    }
}

/// Content analysis results
#[derive(Debug, Clone)]
pub struct ContentAnalysis {
    /// Detected faces with frame ranges
    pub faces: Vec<FaceDetection>,
    /// Detected objects/items
    pub objects: Vec<ObjectDetection>,
    /// Overall content categories
    pub categories: Vec<ContentCategory>,
    /// Audio analysis
    pub audio_analysis: Option<AudioAnalysis>,
}

/// Face detection result
#[derive(Debug, Clone)]
pub struct FaceDetection {
    /// Frame range where face is visible
    pub frame_range: FrameRange,
    /// Bounding box (x, y, width, height) normalized 0-1
    pub bounding_box: (f32, f32, f32, f32),
    /// Confidence score
    pub confidence: f64,
    /// Face ID for tracking same face across frames
    pub face_id: Option<String>,
}

/// Object detection result
#[derive(Debug, Clone)]
pub struct ObjectDetection {
    /// Frame where object is detected
    pub frame: Frame,
    /// Object label
    pub label: String,
    /// Bounding box (x, y, width, height) normalized 0-1
    pub bounding_box: (f32, f32, f32, f32),
    /// Confidence score
    pub confidence: f64,
}

/// Content category
#[derive(Debug, Clone)]
pub struct ContentCategory {
    /// Category name
    pub name: String,
    /// Confidence score
    pub confidence: f64,
}

/// Audio analysis results
#[derive(Debug, Clone)]
pub struct AudioAnalysis {
    /// Segments with speech
    pub speech_segments: Vec<FrameRange>,
    /// Segments with music
    pub music_segments: Vec<FrameRange>,
    /// Segments with silence
    pub silence_segments: Vec<FrameRange>,
    /// Overall loudness (LUFS)
    pub loudness_lufs: f64,
    /// Peak level (dB)
    pub peak_db: f64,
}

/// AI service error types
#[derive(Debug, Clone)]
pub enum AiError {
    /// Model not available
    ModelNotAvailable(String),
    /// Processing failed
    ProcessingFailed(String),
    /// Input media not supported
    UnsupportedMedia(String),
    /// Service unavailable (e.g., API down)
    ServiceUnavailable(String),
    /// Rate limited
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

/// AI features service
///
/// This is a stub implementation. In a real implementation, this would
/// interface with ML models (local or cloud-based).
pub struct AiService {
    /// Whether local models are available
    local_models_available: bool,
}

impl Default for AiService {
    fn default() -> Self {
        Self::new()
    }
}

impl AiService {
    /// Create a new AI service
    pub fn new() -> Self {
        Self {
            local_models_available: false,
        }
    }

    /// Check if scene detection is available
    pub fn is_scene_detection_available(&self) -> bool {
        // Stub: Always available
        true
    }

    /// Check if transcription is available
    pub fn is_transcription_available(&self) -> bool {
        // Stub: Always available
        true
    }

    /// Check if auto-edit suggestions are available
    pub fn is_auto_edit_available(&self) -> bool {
        // Stub: Always available
        true
    }

    /// Detect scene changes in a video asset (stub)
    pub fn detect_scenes(&self, _asset_path: &PathBuf) -> Result<Vec<SceneChange>, AiError> {
        // Stub: Return empty result
        Ok(vec![])
    }

    /// Transcribe audio from a media file (stub)
    pub fn transcribe(&self, _asset_path: &PathBuf) -> Result<Transcript, AiError> {
        // Stub: Return empty transcript
        Ok(Transcript {
            segments: vec![],
            language: None,
            average_confidence: 0.0,
        })
    }

    /// Generate auto-edit suggestions for a timeline (stub)
    pub fn suggest_edits(&self, _timeline: &Timeline) -> Result<Vec<EditSuggestion>, AiError> {
        // Stub: Return empty suggestions
        Ok(vec![])
    }

    /// Analyze content in a video asset (stub)
    pub fn analyze_content(&self, _asset_path: &PathBuf) -> Result<ContentAnalysis, AiError> {
        // Stub: Return empty analysis
        Ok(ContentAnalysis {
            faces: vec![],
            objects: vec![],
            categories: vec![],
            audio_analysis: None,
        })
    }

    /// Detect silence segments in audio (stub)
    pub fn detect_silence(
        &self,
        _asset_path: &PathBuf,
        _threshold_db: f64,
        _min_duration_frames: i64,
    ) -> Result<Vec<FrameRange>, AiError> {
        // Stub: Return empty result
        Ok(vec![])
    }

    /// Check if local models are available
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
                    start: Frame(0),
                    end: Frame(24),
                    text: "Hello".into(),
                    confidence: 0.9,
                    speaker_id: None,
                },
                TranscriptSegment {
                    start: Frame(24),
                    end: Frame(48),
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
                    start: Frame(0),
                    end: Frame(24),
                    text: "The quick brown fox".into(),
                    confidence: 0.9,
                    speaker_id: None,
                },
                TranscriptSegment {
                    start: Frame(24),
                    end: Frame(48),
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
