use llama_cpp_4::context::params::LlamaContextParams;
use llama_cpp_4::context::LlamaContext;
use llama_cpp_4::llama_backend::LlamaBackend;
use llama_cpp_4::llama_batch::LlamaBatch;
use llama_cpp_4::model::params::LlamaModelParams;
use llama_cpp_4::model::LlamaModel;
use llama_cpp_4::model::{AddBos, Special};
use llama_cpp_4::sampling::LlamaSampler;
use llama_cpp_4::token::LlamaToken;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SESSION_CTX: u32 = 2048;
const BATCH_SIZE: u32 = 512;
const OUTPUT_LIMIT: usize = 1024;
const INFERENCE_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// JSON protocol
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
enum HelperRequest {
    /// Pre-decode the system prompt into the KV cache while the user is still
    /// recording. The response is immediate (just an ack).
    #[serde(rename = "prefill")]
    Prefill {
        system_prompt: String,
        model_path: PathBuf,
    },
    /// Run inference. If a prefill was done with a matching system_prompt, the
    /// KV cache is reused and only the user suffix is decoded.
    #[serde(rename = "generate")]
    Generate {
        prompt: String,
        model_path: PathBuf,
        max_tokens: usize,
        temperature: f32,
    },
}

#[derive(Debug, Serialize)]
struct HelperResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl HelperResponse {
    fn ok() -> Self {
        Self { ok: true, text: None, error: None }
    }

    fn ok_text(text: String) -> Self {
        Self { ok: true, text: Some(text), error: None }
    }

    fn err(msg: String) -> Self {
        Self { ok: false, text: None, error: Some(msg) }
    }
}

// ---------------------------------------------------------------------------
// Persistent daemon state
// ---------------------------------------------------------------------------

struct PrefillState {
    system_tokens: Vec<LlamaToken>,
    n_past: i32,
}

/// Bundles the loaded model together with its derived warm context and prefill
/// state.  Because `LlamaContext<'a>` borrows from `LlamaModel`, they must
/// live in the same scope.  We solve the borrow-checker conflict with
/// `ensure_model_loaded` by keeping the model in a `Box` (stable heap address)
/// and dropping the context *before* potentially replacing the model.
///
/// Field order matters for drop: `warm_ctx` and `prefill` are dropped before
/// `model` because Rust drops fields in declaration order.
struct ModelSlot {
    warm_ctx: Option<LlamaContext<'static>>,
    prefill: Option<PrefillState>,
    model: Box<LlamaModel>,
    path: PathBuf,
}

impl ModelSlot {
    /// Load a new model, discarding any previous state.
    fn load(
        backend: &LlamaBackend,
        model_path: &PathBuf,
        n_threads: i32,
    ) -> Option<Self> {
        let model_params = LlamaModelParams::default();
        let model_params = std::pin::pin!(model_params);
        match LlamaModel::load_from_file(backend, model_path, &model_params) {
            Ok(model) => {
                eprintln!(
                    "[pepperx-cleanup-helper] loaded {} ({} threads)",
                    model_path.display(),
                    n_threads,
                );
                Some(ModelSlot {
                    warm_ctx: None,
                    prefill: None,
                    model: Box::new(model),
                    path: model_path.clone(),
                })
            }
            Err(e) => {
                eprintln!("[pepperx-cleanup-helper] failed to load model: {e}");
                None
            }
        }
    }

    /// Get a reference to the model with a lifetime tied to `self`.  We then
    /// erase that lifetime to `'static` when storing the `LlamaContext` in the
    /// slot.  This is sound because:
    ///   1. The `Box<LlamaModel>` has a stable heap address.
    ///   2. We always drop `warm_ctx` before `model` (field order + explicit
    ///      clears whenever we might replace the model).
    fn model_ref_static(&self) -> &'static LlamaModel {
        // SAFETY: The Box guarantees a stable address.  We enforce the
        // invariant that warm_ctx is dropped before model everywhere.
        unsafe { &*(self.model.as_ref() as *const LlamaModel) }
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() {
    // Suppress llama.cpp's own logging.
    unsafe {
        extern "C" fn noop_log(
            _level: llama_cpp_sys_4::ggml_log_level,
            _text: *const std::ffi::c_char,
            _user_data: *mut std::ffi::c_void,
        ) {
        }
        llama_cpp_sys_4::llama_log_set(Some(noop_log), std::ptr::null_mut());
    }

    let backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            write_response(&HelperResponse::err(format!(
                "failed to initialize backend: {e}"
            )));
            std::process::exit(1);
        }
    };

    // P-cores only on Intel hybrid; clamp to [2, 4].
    let n_threads = (num_cpus::get_physical() as i32).min(4).max(2);

    let mut slot: Option<ModelSlot> = None;

    let stdin = io::stdin().lock();
    for line in stdin.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: HelperRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_response(&HelperResponse::err(format!(
                    "invalid request JSON: {e}"
                )));
                continue;
            }
        };

        // Extract model_path before we move `request` into the handler.
        let request_model_path = match &request {
            HelperRequest::Prefill { model_path, .. } => model_path.clone(),
            HelperRequest::Generate { model_path, .. } => model_path.clone(),
        };

        // Ensure the right model is loaded.  If the model path changed, drop
        // the entire slot (context + prefill + model) and reload.
        let needs_load = match slot.as_ref() {
            Some(s) => s.path != request_model_path,
            None => true,
        };
        if needs_load {
            // Drop old slot first (drops warm_ctx before model).
            drop(slot.take());
            slot = ModelSlot::load(&backend, &request_model_path, n_threads);
        }

        let s = match slot.as_mut() {
            Some(s) => s,
            None => {
                write_response(&HelperResponse::err("model not loaded".into()));
                continue;
            }
        };

        let model = s.model_ref_static();
        let response = handle_request(
            request,
            model,
            &backend,
            n_threads,
            &mut s.prefill,
            &mut s.warm_ctx,
        );
        write_response(&response);
    }
}

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

/// Dispatch a single request.  Takes `&LlamaModel` directly (already resolved
/// from the slot by the caller) so there is no borrow-checker conflict with
/// model loading.
fn handle_request(
    request: HelperRequest,
    model: &'static LlamaModel,
    backend: &LlamaBackend,
    n_threads: i32,
    prefill_state: &mut Option<PrefillState>,
    warm_ctx: &mut Option<LlamaContext<'static>>,
) -> HelperResponse {
    match request {
        HelperRequest::Prefill {
            system_prompt,
            model_path: _,
        } => handle_prefill(model, backend, n_threads, &system_prompt, prefill_state, warm_ctx),

        HelperRequest::Generate {
            prompt,
            model_path: _,
            max_tokens,
            temperature,
        } => handle_generate(
            model,
            backend,
            n_threads,
            &prompt,
            max_tokens,
            temperature,
            prefill_state,
            warm_ctx,
        ),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_ctx_params(n_threads: i32) -> LlamaContextParams {
    LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(SESSION_CTX))
        .with_n_batch(BATCH_SIZE)
        .with_n_threads(n_threads)
        .with_n_threads_batch(n_threads)
}

/// Create a fresh `LlamaContext`, returning a `HelperResponse` error on failure.
fn new_context(
    model: &'static LlamaModel,
    backend: &LlamaBackend,
    n_threads: i32,
) -> Result<LlamaContext<'static>, HelperResponse> {
    let ctx_params = make_ctx_params(n_threads);
    model
        .new_context(backend, ctx_params)
        .map_err(|e| HelperResponse::err(format!("failed to create context: {e}")))
}

// ---------------------------------------------------------------------------
// Prefill
// ---------------------------------------------------------------------------

fn handle_prefill(
    model: &'static LlamaModel,
    backend: &LlamaBackend,
    n_threads: i32,
    system_prompt: &str,
    prefill_state: &mut Option<PrefillState>,
    warm_ctx: &mut Option<LlamaContext<'static>>,
) -> HelperResponse {
    // Reuse existing context if available, just clear the KV cache.
    // Context creation is extremely expensive (~3.5s) on hybrid Mamba models.
    *prefill_state = None;

    let mut ctx = match warm_ctx.take() {
        Some(mut existing) => {
            existing.clear_kv_cache();
            existing
        }
        None => match new_context(model, backend, n_threads) {
            Ok(c) => c,
            Err(resp) => return resp,
        },
    };

    let tokens = match model.str_to_token(system_prompt, AddBos::Always) {
        Ok(t) => t,
        Err(e) => return HelperResponse::err(format!("prefill tokenization failed: {e}")),
    };

    let mut batch = LlamaBatch::new(tokens.len().max(BATCH_SIZE as usize), 1);
    for (i, token) in tokens.iter().enumerate() {
        let _ = batch.add(*token, i as i32, &[0], i == tokens.len() - 1);
    }

    let t0 = Instant::now();
    if let Err(e) = ctx.decode(&mut batch) {
        return HelperResponse::err(format!("prefill decode failed: {e}"));
    }

    let n_past = tokens.len() as i32;
    eprintln!(
        "[cleanup-helper] prefilled {} tokens in {}ms",
        tokens.len(),
        t0.elapsed().as_millis()
    );

    *prefill_state = Some(PrefillState {
        system_tokens: tokens,
        n_past,
    });
    *warm_ctx = Some(ctx);

    HelperResponse::ok()
}

// ---------------------------------------------------------------------------
// Generate
// ---------------------------------------------------------------------------

fn handle_generate(
    model: &'static LlamaModel,
    backend: &LlamaBackend,
    n_threads: i32,
    prompt: &str,
    max_tokens: usize,
    temperature: f32,
    prefill_state: &mut Option<PrefillState>,
    warm_ctx: &mut Option<LlamaContext<'static>>,
) -> HelperResponse {
    let t0 = Instant::now();

    let all_tokens = match model.str_to_token(prompt, AddBos::Always) {
        Ok(t) => t,
        Err(e) => return HelperResponse::err(format!("tokenization failed: {e}")),
    };

    // Try to reuse the prefilled KV cache.
    let (mut ctx, n_past) = match (prefill_state.take(), warm_ctx.take()) {
        (Some(ps), Some(wc)) => {
            let prefix_match = all_tokens.len() >= ps.system_tokens.len()
                && all_tokens[..ps.system_tokens.len()] == ps.system_tokens[..];
            if prefix_match {
                (wc, ps.n_past)
            } else {
                // Prefix mismatch -- drop warm context, create fresh.
                drop(wc);
                match new_context(model, backend, n_threads) {
                    Ok(c) => (c, 0),
                    Err(resp) => return resp,
                }
            }
        }
        // Reuse warm context if available (avoids expensive context creation).
        (_, Some(mut wc)) => {
            wc.clear_kv_cache();
            (wc, 0)
        }
        // No context at all — create fresh (expensive, first call only).
        (_, None) => {
            match new_context(model, backend, n_threads) {
                Ok(c) => (c, 0),
                Err(resp) => return resp,
            }
        }
    };

    // Decode only the suffix tokens (everything after the prefilled prefix).
    let suffix_tokens = &all_tokens[n_past as usize..];
    if !suffix_tokens.is_empty() {
        let mut batch = LlamaBatch::new(suffix_tokens.len().max(BATCH_SIZE as usize), 1);
        for (i, token) in suffix_tokens.iter().enumerate() {
            let pos = n_past + i as i32;
            let is_last = i == suffix_tokens.len() - 1;
            let _ = batch.add(*token, pos, &[0], is_last);
        }

        if let Err(e) = ctx.decode(&mut batch) {
            return HelperResponse::err(format!("suffix decode failed: {e}"));
        }
    }

    let decode_ms = t0.elapsed().as_millis();

    // --- Autoregressive generation ---
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::temp(temperature),
        LlamaSampler::dist(1),
    ]);

    let deadline = Instant::now() + INFERENCE_TIMEOUT;
    let mut generated = String::new();
    let mut n_cur = n_past + suffix_tokens.len() as i32;
    let mut n_generated = 0usize;

    // For the first iteration we sample from the logits left by the decode
    // above (the suffix decode, or the prefill decode if the suffix was empty).
    // On subsequent iterations we decode the previously sampled token first.
    let mut batch = LlamaBatch::new(1, 1);
    let mut first_iter = true;

    for _ in 0..max_tokens {
        if !first_iter {
            if ctx.decode(&mut batch).is_err() {
                break;
            }
        }
        first_iter = false;

        let idx = batch.n_tokens().saturating_sub(1);
        let token = sampler.sample(&ctx, idx);
        sampler.accept(token);

        if model.is_eog_token(token) {
            break;
        }

        let piece = match model.token_to_str(token, Special::Tokenize) {
            Ok(p) => p,
            Err(_) => break,
        };
        generated.push_str(&piece);

        // Stop on newline (ignoring <think> blocks the model may emit).
        let stripped = strip_think_blocks(&generated);
        if stripped.contains('\n') || stripped.len() >= OUTPUT_LIMIT {
            break;
        }
        if Instant::now() > deadline {
            break;
        }

        batch.clear();
        let _ = batch.add(token, n_cur, &[0], true);
        n_cur += 1;
        n_generated += 1;
    }

    let total_ms = t0.elapsed().as_millis();
    eprintln!(
        "[cleanup-helper] {}tok total, {}tok prefilled, {}tok suffix, {}tok out, decode={}ms total={}ms",
        all_tokens.len(), n_past, suffix_tokens.len(), n_generated, decode_ms, total_ms
    );

    // Save the context back so the next prefill can reuse it (avoiding
    // expensive context creation on the next recording).
    *warm_ctx = Some(ctx);

    HelperResponse::ok_text(generated)
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn write_response(response: &HelperResponse) {
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, response);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

fn strip_think_blocks(text: &str) -> String {
    let mut result = text.to_string();
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
            // Unclosed <think> -- strip everything from the tag onward.
            result = result[..open_start].to_string();
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
