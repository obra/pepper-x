use llama_cpp::standard_sampler::StandardSampler;
use llama_cpp::{LlamaModel, LlamaParams, SessionParams};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

const CLEANUP_BACKEND_NAME: &str = "llama.cpp";
const CLEANUP_MAX_TOKENS: usize = 128;
const CLEANUP_OUTPUT_LIMIT: usize = 512;
const CLEANUP_OCR_CONTEXT_LIMIT: usize = 512;
const CLEANUP_SESSION_CTX: u32 = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupRequest {
    pub transcript_text: String,
    pub model_path: PathBuf,
    pub ocr_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupResult {
    pub backend_name: String,
    pub model_name: String,
    pub cleaned_text: String,
    pub elapsed_ms: u64,
    pub used_ocr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupError {
    MissingModelConfiguration,
    MissingModelPath(PathBuf),
    EmptyTranscript,
    LoadModel {
        model_path: PathBuf,
        message: String,
    },
    CreateSession {
        model_name: String,
        message: String,
    },
    AdvanceContext {
        model_name: String,
        message: String,
    },
    EmptyCompletion {
        model_name: String,
    },
}

impl CleanupError {
    pub fn backend_name(&self) -> &'static str {
        CLEANUP_BACKEND_NAME
    }

    pub fn model_name(&self) -> Option<String> {
        match self {
            Self::MissingModelConfiguration | Self::MissingModelPath(_) | Self::EmptyTranscript => {
                None
            }
            Self::LoadModel { model_path, .. } => model_name_from_path(model_path),
            Self::CreateSession { model_name, .. }
            | Self::AdvanceContext { model_name, .. }
            | Self::EmptyCompletion { model_name } => Some(model_name.clone()),
        }
    }
}

impl std::error::Error for CleanupError {}

impl fmt::Display for CleanupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingModelConfiguration => f.write_str("cleanup model path is not configured"),
            Self::MissingModelPath(model_path) => write!(
                f,
                "cleanup model path does not exist: {}",
                model_path.display()
            ),
            Self::EmptyTranscript => f.write_str("cleanup transcript is empty"),
            Self::LoadModel {
                model_path,
                message,
            } => write!(
                f,
                "failed to load cleanup model {}: {message}",
                model_path.display()
            ),
            Self::CreateSession {
                model_name,
                message,
            } => write!(
                f,
                "failed to create cleanup session for {model_name}: {message}"
            ),
            Self::AdvanceContext {
                model_name,
                message,
            } => write!(
                f,
                "failed to prepare cleanup context for {model_name}: {message}"
            ),
            Self::EmptyCompletion { model_name } => {
                write!(f, "cleanup model {model_name} returned no text")
            }
        }
    }
}

pub fn cleanup_prompt(request: &CleanupRequest) -> String {
    let mut prompt = String::from(
        "You clean speech recognition transcripts.\n\
Return only the cleaned transcript on a single line.\n\
Preserve wording and meaning.\n\
Fix capitalization, punctuation, and obvious transcription artifacts.\n",
    );

    if let Some(ocr_text) = bounded_ocr_text(request.ocr_text.as_deref()) {
        prompt.push_str("Optional OCR context:\n");
        prompt.push_str(&ocr_text);
        prompt.push_str("\n");
    }

    prompt.push_str("Raw transcript:\n");
    prompt.push_str(request.transcript_text.trim());
    prompt.push_str("\nCleaned transcript:\n");
    prompt
}

pub fn run_cleanup(request: &CleanupRequest) -> Result<CleanupResult, CleanupError> {
    if request.transcript_text.trim().is_empty() {
        return Err(CleanupError::EmptyTranscript);
    }

    if request.model_path.as_os_str().is_empty() {
        return Err(CleanupError::MissingModelConfiguration);
    }

    if !request.model_path.exists() {
        return Err(CleanupError::MissingModelPath(request.model_path.clone()));
    }

    let model_name =
        model_name_from_path(&request.model_path).unwrap_or_else(|| String::from("unknown"));
    let start = Instant::now();
    let model = LlamaModel::load_from_file(&request.model_path, LlamaParams::default()).map_err(
        |error| CleanupError::LoadModel {
            model_path: request.model_path.clone(),
            message: error.to_string(),
        },
    )?;
    let mut session_params = SessionParams::default();
    session_params.seed = 1;
    if session_params.n_ctx < CLEANUP_SESSION_CTX {
        session_params.n_ctx = CLEANUP_SESSION_CTX;
    }

    let mut session =
        model
            .create_session(session_params)
            .map_err(|error| CleanupError::CreateSession {
                model_name: model_name.clone(),
                message: error.to_string(),
            })?;
    session
        .advance_context(cleanup_prompt(request))
        .map_err(|error| CleanupError::AdvanceContext {
            model_name: model_name.clone(),
            message: error.to_string(),
        })?;

    let mut generated = String::new();
    let completions = session
        .start_completing_with(StandardSampler::new_greedy(), CLEANUP_MAX_TOKENS)
        .map_err(|error| CleanupError::AdvanceContext {
            model_name: model_name.clone(),
            message: error.to_string(),
        })?
        .into_strings();

    for piece in completions {
        generated.push_str(&piece);

        if generated.contains('\n') || generated.len() >= CLEANUP_OUTPUT_LIMIT {
            break;
        }
    }

    let cleaned_text = normalize_cleanup_output(&generated);
    if cleaned_text.is_empty() {
        return Err(CleanupError::EmptyCompletion { model_name });
    }

    Ok(CleanupResult {
        backend_name: CLEANUP_BACKEND_NAME.into(),
        model_name,
        cleaned_text,
        elapsed_ms: start.elapsed().as_millis() as u64,
        used_ocr: bounded_ocr_text(request.ocr_text.as_deref()).is_some(),
    })
}

fn bounded_ocr_text(ocr_text: Option<&str>) -> Option<String> {
    let trimmed = ocr_text?.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.chars().take(CLEANUP_OCR_CONTEXT_LIMIT).collect())
}

fn model_name_from_path(model_path: &Path) -> Option<String> {
    model_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn normalize_cleanup_output(output: &str) -> String {
    let first_line = output
        .lines()
        .next()
        .unwrap_or(output)
        .trim()
        .trim_matches('"')
        .trim();

    first_line
        .strip_prefix("Cleaned transcript:")
        .unwrap_or(first_line)
        .trim()
        .to_string()
}
