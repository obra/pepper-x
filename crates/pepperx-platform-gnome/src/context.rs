#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SupportingContext {
    pub supporting_context_text: Option<String>,
    pub ocr_text: Option<String>,
    pub used_ocr: bool,
}

#[allow(dead_code)]
pub(crate) fn supporting_context_from_atspi(
    snapshot: Option<&super::FocusedTargetSnapshot>,
    ocr_text: Option<&str>,
    max_chars: usize,
) -> SupportingContext {
    if let Some(snapshot) = snapshot {
        if let Some(supporting_context_text) = snapshot.supporting_context_text(max_chars) {
            return SupportingContext {
                supporting_context_text: Some(supporting_context_text),
                ocr_text: None,
                used_ocr: false,
            };
        }
    }

    let ocr_text = ocr_text.map(|text| bound_text(text, max_chars));
    let used_ocr = ocr_text.is_some();

    SupportingContext {
        supporting_context_text: ocr_text.clone(),
        ocr_text,
        used_ocr,
    }
}

#[allow(dead_code)]
fn bound_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars.max(1)).collect()
}

#[cfg(test)]
mod tests {
    use super::supporting_context_from_atspi;
    use crate::atspi::FocusedTargetSnapshot;

    #[test]
    fn context_prefers_atspi_text_over_ocr_and_bounds_it() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.TextEditor".into(),
            application_name: "Text Editor".into(),
            target_class: "text-editor",
            is_editable: true,
            supports_text: true,
            supports_editable_text: true,
            supports_caret: true,
            before_text: Some("abcdefghi".into()),
            caret_offset: Some(4),
        };

        let context = supporting_context_from_atspi(Some(&snapshot), Some("ocr fallback"), 4);

        assert_eq!(context.supporting_context_text.as_deref(), Some("cdef"));
        assert_eq!(context.ocr_text, None);
        assert!(!context.used_ocr);
    }

    #[test]
    fn context_uses_ocr_when_atspi_text_is_unavailable() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.TextEditor".into(),
            application_name: "Text Editor".into(),
            target_class: "text-editor",
            is_editable: true,
            supports_text: false,
            supports_editable_text: false,
            supports_caret: false,
            before_text: None,
            caret_offset: None,
        };

        let context = supporting_context_from_atspi(Some(&snapshot), Some("ocr fallback"), 4);

        assert_eq!(context.supporting_context_text.as_deref(), Some("ocr "));
        assert_eq!(context.ocr_text.as_deref(), Some("ocr "));
        assert!(context.used_ocr);
    }

    #[test]
    fn context_leaves_used_ocr_false_without_ocr_input() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.TextEditor".into(),
            application_name: "Text Editor".into(),
            target_class: "text-editor",
            is_editable: true,
            supports_text: false,
            supports_editable_text: false,
            supports_caret: false,
            before_text: None,
            caret_offset: None,
        };

        let context = supporting_context_from_atspi(Some(&snapshot), None, 4);

        assert_eq!(context.supporting_context_text, None);
        assert_eq!(context.ocr_text, None);
        assert!(!context.used_ocr);
    }
}
