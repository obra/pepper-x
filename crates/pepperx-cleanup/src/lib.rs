pub mod cleanup;

pub use cleanup::{
    cleanup_prompt, run_cleanup, CleanupError, CleanupRequest, CleanupResult,
    LITERAL_DICTATION_PROMPT_PROFILE, ORDINARY_DICTATION_PROMPT_PROFILE,
};

#[cfg(test)]
mod cleanup_runtime {
    use super::{cleanup_prompt, run_cleanup, CleanupError, CleanupRequest};
    use crate::cleanup::ORDINARY_DICTATION_PROMPT_PROFILE;
    use std::path::PathBuf;

    #[test]
    fn cleanup_runtime_rejects_missing_model_path() {
        let error = run_cleanup(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-missing.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
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
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
        };

        assert_eq!(cleanup_prompt(&request), cleanup_prompt(&request));
    }

    #[test]
    fn cleanup_runtime_prompt_profile_changes_the_prompt_contract() {
        let ordinary_prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
        });
        let literal_prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: "literal-dictation".into(),
        });

        assert_ne!(ordinary_prompt, literal_prompt);
        assert!(literal_prompt.contains("Preserve spoken filler words"));
    }

    #[test]
    fn cleanup_ocr_prompt_omits_context_when_absent() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
        });

        assert!(!prompt.contains("Optional OCR context:"));
    }

    #[test]
    fn cleanup_ocr_prompt_bounds_context_when_present() {
        let oversized_ocr = "A".repeat(640);
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: Some(oversized_ocr.clone()),
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
        });

        let bounded_ocr = "A".repeat(512);

        assert!(prompt.contains("Optional OCR context:\n"));
        assert!(prompt.contains(&bounded_ocr));
        assert!(!prompt.contains(&oversized_ocr));
    }

    #[test]
    fn cleanup_ocr_prompt_includes_supporting_context_without_marking_it_as_ocr() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: Some("line before\nline after".into()),
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
        });

        assert!(prompt.contains("Optional supporting context:\nline before\nline after"));
        assert!(!prompt.contains("Optional OCR context:"));
    }

    #[test]
    fn cleanup_runtime_prompt_includes_correction_memory_when_present() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: Some(
                "When the raw transcript is `pepper x`, prefer `Pepper X`.".into(),
            ),
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
        });

        assert!(prompt.contains("Saved correction memory:\n"));
        assert!(prompt.contains("When the raw transcript is `pepper x`, prefer `Pepper X`."));
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
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
        })
        .expect("real cleanup run should succeed");

        assert!(!result.cleaned_text.trim().is_empty());
    }
}
