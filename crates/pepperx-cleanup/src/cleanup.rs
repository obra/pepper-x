use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CLEANUP_BACKEND_NAME: &str = "llama.cpp";
pub const ORDINARY_DICTATION_PROMPT_PROFILE: &str = "ordinary-dictation";
pub const LITERAL_DICTATION_PROMPT_PROFILE: &str = "literal-dictation";
const CLEANUP_MAX_TOKENS: usize = 256;
const CLEANUP_OCR_CONTEXT_LIMIT: usize = 4000;
const CLEANUP_CORRECTION_MEMORY_LIMIT: usize = 2048;
const CLEANUP_CUSTOM_PROMPT_LIMIT: usize = 2048;
const CLEANUP_TEMPERATURE: f32 = 0.1;
const CLEANUP_SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(30);

const DEFAULT_CLEANUP_HELPER_BIN: &str = "/usr/libexec/pepper-x/pepperx-cleanup-helper";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupRequest {
    pub transcript_text: String,
    pub model_path: PathBuf,
    pub supporting_context_text: Option<String>,
    pub ocr_text: Option<String>,
    pub correction_memory_text: Option<String>,
    pub prompt_profile: String,
    pub custom_prompt_text: Option<String>,
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
    UnsupportedModel(String),
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
    SubprocessError {
        message: String,
    },
}

impl CleanupError {
    pub fn backend_name(&self) -> &'static str {
        CLEANUP_BACKEND_NAME
    }

    pub fn model_name(&self) -> Option<String> {
        match self {
            Self::MissingModelConfiguration
            | Self::UnsupportedModel(_)
            | Self::MissingModelPath(_)
            | Self::EmptyTranscript
            | Self::SubprocessError { .. } => None,
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
            Self::UnsupportedModel(model_id) => {
                write!(f, "cleanup model is not supported: {model_id}")
            }
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
            Self::SubprocessError { message } => {
                write!(f, "cleanup helper subprocess failed: {message}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Subprocess protocol (shared with pepperx-cleanup-helper)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct PrefillRequest {
    action: &'static str,
    system_prompt: String,
    model_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct GenerateRequest {
    action: &'static str,
    prompt: String,
    model_path: PathBuf,
    max_tokens: usize,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct CleanupHelperResponse {
    ok: bool,
    text: Option<String>,
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// Prompt assembly
// ---------------------------------------------------------------------------

pub fn cleanup_prompt(request: &CleanupRequest) -> String {
    let mut sections: Vec<String> = Vec::new();

    // Section 1: Base prompt (user custom or built-in default)
    let mut base = prompt_preamble(&request.prompt_profile).to_string();
    if let Some(custom_prompt_text) = custom_prompt_text(request.custom_prompt_text.as_deref()) {
        base.push_str(&custom_prompt_text);
    }
    sections.push(base);

    // Section 2: Correction hints
    if let Some(correction_text) =
        bounded_correction_memory_text(request.correction_memory_text.as_deref())
    {
        sections.push(format!(
            "<CORRECTION-HINTS>\n{correction_text}\n</CORRECTION-HINTS>"
        ));
    }

    // Sections 3 & 4: OCR rules + window OCR content
    let ocr_text = bounded_supporting_context_text(request.supporting_context_text.as_deref())
        .or_else(|| bounded_ocr_text(request.ocr_text.as_deref()));
    if let Some(ocr_text) = &ocr_text {
        sections.push(
            "<OCR-RULES>\n\
             The following text was captured from the user's screen. Use it ONLY as \
             disambiguation context: prefer the user's spoken words, but correct likely \
             misrecognitions of names, commands, files, and jargon visible on screen. \
             Never summarize or rewrite the window content.\n\
             </OCR-RULES>"
                .to_string(),
        );
        sections.push(format!(
            "<WINDOW-OCR-CONTENT>\n{ocr_text}\n</WINDOW-OCR-CONTENT>"
        ));
    }

    let system_prompt = sections.join("\n\n");
    let transcript = request.transcript_text.trim();
    format_chat_prompt(&system_prompt, transcript)
}

/// Build just the system prompt portion (for prefilling the KV cache).
pub fn cleanup_system_prompt(request: &CleanupRequest) -> String {
    let mut sections: Vec<String> = Vec::new();
    let mut base = prompt_preamble(&request.prompt_profile).to_string();
    if let Some(custom_prompt_text) = custom_prompt_text(request.custom_prompt_text.as_deref()) {
        base.push_str(&custom_prompt_text);
    }
    sections.push(base);
    if let Some(correction_text) =
        bounded_correction_memory_text(request.correction_memory_text.as_deref())
    {
        sections.push(format!(
            "<CORRECTION-HINTS>\n{correction_text}\n</CORRECTION-HINTS>"
        ));
    }
    let ocr_text = bounded_supporting_context_text(request.supporting_context_text.as_deref())
        .or_else(|| bounded_ocr_text(request.ocr_text.as_deref()));
    if let Some(ocr_text) = &ocr_text {
        sections.push(
            "<OCR-RULES>\n\
             The following text was captured from the user's screen. Use it ONLY as \
             disambiguation context: prefer the user's spoken words, but correct likely \
             misrecognitions of names, commands, files, and jargon visible on screen. \
             Never summarize or rewrite the window content.\n\
             </OCR-RULES>"
                .to_string(),
        );
        sections.push(format!(
            "<WINDOW-OCR-CONTENT>\n{ocr_text}\n</WINDOW-OCR-CONTENT>"
        ));
    }
    let system_prompt = sections.join("\n\n");
    // Return the system portion of the chat template (up to and including <|im_end|>
    // and the start of the user turn). The prefill decodes this into KV cache.
    format!("<|im_start|>system\n{system_prompt}<|im_end|>\n<|im_start|>user\n")
}

fn format_chat_prompt(system_prompt: &str, transcript: &str) -> String {
    format!(
        "<|im_start|>system\n{system_prompt}<|im_end|>\n\
         <|im_start|>user\n<USER-INPUT>\n{transcript}\n</USER-INPUT><|im_end|>\n\
         <|im_start|>assistant\n"
    )
}

fn prompt_preamble(profile: &str) -> &'static str {
    match profile {
        ORDINARY_DICTATION_PROMPT_PROFILE => ORDINARY_DICTATION_PREAMBLE,
        LITERAL_DICTATION_PROMPT_PROFILE => LITERAL_DICTATION_PREAMBLE,
        _ => ORDINARY_DICTATION_PREAMBLE,
    }
}

const ORDINARY_DICTATION_PREAMBLE: &str = "/no_think
Clean up this speech transcript. Return ONLY the cleaned text on one line.
Delete fillers (um, uh, like, you know, basically, literally, sort of, kind of). Handle \"scratch that\"/\"never mind\" by deleting what's being corrected. Fix punctuation. Fix obvious misrecognitions. Keep everything else exactly as spoken.
Example: \"so um I was thinking we should uh move the deadline to Friday\" -> \"I was thinking we should move the deadline to Friday.\"
Example: \"update the DNS record for the scratch that update the SSL cert for api dot example dot com\" -> \"Update the SSL certificate for api.example.com.\"
";

const LITERAL_DICTATION_PREAMBLE: &str = "/no_think
You lightly normalize speech-recognition transcripts. Return ONLY the transcript on a single line.

Rules:
- Preserve spoken filler words, hesitations, and casing when they appear intentional.
- Fix only obvious transcription errors that change the words themselves.
- Properly punctuate sentences.

";

// ---------------------------------------------------------------------------
// Inference (via subprocess)
// ---------------------------------------------------------------------------

/// Pre-decode the system prompt into the cleanup helper's KV cache.
/// Call this when recording starts so the cache is warm when cleanup runs.
pub fn prefill_cleanup_system_prompt(request: &CleanupRequest) {
    if request.model_path.as_os_str().is_empty() || !request.model_path.exists() {
        return;
    }

    let system_prompt = cleanup_system_prompt(request);
    let prefill = PrefillRequest {
        action: "prefill",
        system_prompt,
        model_path: request.model_path.clone(),
    };

    let Ok(json) = serde_json::to_string(&prefill) else {
        return;
    };

    eprintln!("[Pepper X] sending cleanup prefill ({} chars)", json.len());
    let _ = send_to_helper(&json);
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

    let prompt = cleanup_prompt(request);

    let helper_request = GenerateRequest {
        action: "generate",
        prompt,
        model_path: request.model_path.clone(),
        max_tokens: CLEANUP_MAX_TOKENS,
        temperature: CLEANUP_TEMPERATURE,
    };

    let request_json = serde_json::to_string(&helper_request).map_err(|error| {
        CleanupError::SubprocessError {
            message: format!("failed to serialize helper request: {error}"),
        }
    })?;

    let generated = spawn_cleanup_helper(&request_json, &model_name)?;

    let cleaned_text = normalize_cleanup_output(&generated);
    if cleaned_text.is_empty() || cleaned_text == "..." {
        // Fall back to raw transcript when model output is unusable
        let fallback = request.transcript_text.trim().to_string();
        return Ok(CleanupResult {
            backend_name: CLEANUP_BACKEND_NAME.into(),
            model_name,
            cleaned_text: fallback,
            elapsed_ms: start.elapsed().as_millis() as u64,
            used_ocr: has_ocr_context(request),
        });
    }

    Ok(CleanupResult {
        backend_name: CLEANUP_BACKEND_NAME.into(),
        model_name,
        cleaned_text,
        elapsed_ms: start.elapsed().as_millis() as u64,
        used_ocr: has_ocr_context(request),
    })
}

fn configured_cleanup_helper_bin_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PEPPERX_CLEANUP_HELPER_BIN") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    // Look next to the current executable first (for development builds).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("pepperx-cleanup-helper");
            if sibling.is_file() {
                return sibling;
            }
        }
    }

    PathBuf::from(DEFAULT_CLEANUP_HELPER_BIN)
}

/// Send a JSON message to the persistent helper and read one JSON response line.
fn send_to_helper(request_json: &str) -> Result<CleanupHelperResponse, CleanupError> {
    let response_line = send_to_helper_raw(request_json)?;
    serde_json::from_str(&response_line).map_err(|error| CleanupError::SubprocessError {
        message: format!("cleanup helper returned invalid JSON: {error}"),
    })
}

fn spawn_cleanup_helper(
    request_json: &str,
    model_name: &str,
) -> Result<String, CleanupError> {
    let response = send_to_helper(request_json)?;

    if !response.ok {
        let error_message = response.error.unwrap_or_else(|| "unknown error".into());
        if error_message.contains("failed to load model") {
            return Err(CleanupError::LoadModel {
                model_path: PathBuf::from(model_name),
                message: error_message,
            });
        }
        if error_message.contains("failed to create context") {
            return Err(CleanupError::CreateSession {
                model_name: model_name.into(),
                message: error_message,
            });
        }
        return Err(CleanupError::SubprocessError {
            message: error_message,
        });
    }

    response
        .text
        .ok_or_else(|| CleanupError::EmptyCompletion {
            model_name: model_name.into(),
        })
}

fn send_to_helper_raw(request_json: &str) -> Result<String, CleanupError> {
    use std::sync::Mutex;

    static HELPER: Mutex<Option<std::process::Child>> = Mutex::new(None);

    let helper_bin = configured_cleanup_helper_bin_path();
    let mut guard = HELPER.lock().unwrap();

    let child = match guard.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(Some(_)) => {
                *guard = None;
                None
            }
            Ok(None) => Some(child),
            Err(_) => {
                *guard = None;
                None
            }
        },
        None => None,
    };

    if child.is_none() {
        let new_child = Command::new(&helper_bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // TODO: change to Stdio::null() for release
            .spawn()
            .map_err(|error| CleanupError::SubprocessError {
                message: format!(
                    "failed to spawn cleanup helper {}: {error}",
                    helper_bin.display()
                ),
            })?;
        *guard = Some(new_child);
    }

    let child = guard.as_mut().unwrap();

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| CleanupError::SubprocessError {
            message: "cleanup helper stdin is not available".into(),
        })?;
    stdin
        .write_all(request_json.as_bytes())
        .map_err(|error| CleanupError::SubprocessError {
            message: format!("failed to write to cleanup helper: {error}"),
        })?;
    stdin.write_all(b"\n").map_err(|error| {
        CleanupError::SubprocessError {
            message: format!("failed to write newline to cleanup helper: {error}"),
        }
    })?;
    stdin.flush().map_err(|error| CleanupError::SubprocessError {
        message: format!("failed to flush cleanup helper stdin: {error}"),
    })?;

    let stdout = child
        .stdout
        .as_mut()
        .ok_or_else(|| CleanupError::SubprocessError {
            message: "cleanup helper stdout is not available".into(),
        })?;
    let mut reader = io::BufReader::new(stdout);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|error| CleanupError::SubprocessError {
            message: format!("failed to read from cleanup helper: {error}"),
        })?;

    if response_line.is_empty() {
        *guard = None;
        return Err(CleanupError::SubprocessError {
            message: "cleanup helper exited unexpectedly".into(),
        });
    }

    Ok(response_line)
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Child has exited; collect output.
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    std::io::Read::read_to_end(&mut out, &mut stdout).ok();
                }
                if let Some(mut err) = child.stderr.take() {
                    std::io::Read::read_to_end(&mut err, &mut stderr).ok();
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    return Err(format!(
                        "cleanup helper timed out after {timeout:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                return Err(format!("failed to wait on cleanup helper: {error}"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Output sanitization
// ---------------------------------------------------------------------------

fn normalize_cleanup_output(output: &str) -> String {
    let sanitized = strip_reasoning_tags(output);

    let first_line = sanitized
        .lines()
        .next()
        .unwrap_or(&sanitized)
        .trim()
        .trim_matches('"')
        .trim();

    first_line
        .strip_prefix("Cleaned transcript:")
        .unwrap_or(first_line)
        .trim()
        .to_string()
}

/// Strip matched `<think>...</think>` blocks and orphan leading `<think>` tags.
pub(crate) fn strip_reasoning_tags(output: &str) -> String {
    let mut result = output.to_string();

    // Remove matched think blocks (case-insensitive, with optional attributes)
    loop {
        let lower = result.to_lowercase();
        let Some(open_start) = lower.find("<think") else {
            break;
        };
        let Some(open_end) = result[open_start..].find('>') else {
            break;
        };
        let open_end = open_start + open_end + 1;
        let Some(close_start) = lower[open_end..].find("</think") else {
            // Orphan opening tag at the start -- remove everything up to the
            // first newline after the tag (the model's internal reasoning).
            if result[..open_start].trim().is_empty() {
                let after_tag = &result[open_end..];
                let skip = after_tag.find('\n').map(|p| p + 1).unwrap_or(after_tag.len());
                result = after_tag[skip..].to_string();
            }
            break;
        };
        let close_start = open_end + close_start;
        let Some(close_end) = result[close_start..].find('>') else {
            break;
        };
        let close_end = close_start + close_end + 1;
        result = format!("{}{}", &result[..open_start], &result[close_end..]);
    }

    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn has_ocr_context(request: &CleanupRequest) -> bool {
    bounded_supporting_context_text(request.supporting_context_text.as_deref()).is_some()
        || bounded_ocr_text(request.ocr_text.as_deref()).is_some()
}

fn bounded_supporting_context_text(supporting_context_text: Option<&str>) -> Option<String> {
    let trimmed = supporting_context_text?.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.chars().take(CLEANUP_OCR_CONTEXT_LIMIT).collect())
}

fn bounded_correction_memory_text(correction_memory_text: Option<&str>) -> Option<String> {
    let trimmed = correction_memory_text?.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(
        trimmed
            .chars()
            .take(CLEANUP_CORRECTION_MEMORY_LIMIT)
            .collect(),
    )
}

fn custom_prompt_text(custom_prompt_text: Option<&str>) -> Option<String> {
    let custom_prompt_text = custom_prompt_text?;
    if !custom_prompt_text
        .chars()
        .any(|character| !character.is_whitespace())
    {
        return None;
    }

    let mut prompt: String = custom_prompt_text
        .chars()
        .take(CLEANUP_CUSTOM_PROMPT_LIMIT)
        .collect();
    if !prompt.ends_with('\n') {
        prompt.push('\n');
    }
    Some(prompt)
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
