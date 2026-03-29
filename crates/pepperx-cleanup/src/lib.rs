pub mod cleanup;

pub use cleanup::{
    cleanup_prompt, run_cleanup, CleanupError, CleanupRequest, CleanupResult,
};

#[cfg(test)]
mod cleanup_runtime {
    use super::{cleanup_prompt, run_cleanup, CleanupError, CleanupRequest};
    use std::path::PathBuf;

    #[test]
    fn cleanup_runtime_rejects_missing_model_path() {
        let error = run_cleanup(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-missing.gguf"),
            ocr_text: None,
        })
        .unwrap_err();

        assert_eq!(
            error,
            CleanupError::MissingModelPath(PathBuf::from("/tmp/pepper-x-missing.gguf"))
        );
    }

    #[test]
    fn cleanup_runtime_prompt_is_deterministic_for_same_transcript_input() {
        let request = CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            ocr_text: None,
        };

        assert_eq!(cleanup_prompt(&request), cleanup_prompt(&request));
    }

    #[test]
    #[ignore = "requires a real cleanup model"]
    fn cleanup_real_runs_against_a_real_model() {
        let model_path = std::env::var_os("PEPPERX_CLEANUP_MODEL_PATH")
            .map(PathBuf::from)
            .expect("PEPPERX_CLEANUP_MODEL_PATH must point to a GGUF cleanup model");

        let result = run_cleanup(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path,
            ocr_text: None,
        })
        .expect("real cleanup run should succeed");

        assert!(!result.cleaned_text.trim().is_empty());
    }
}
