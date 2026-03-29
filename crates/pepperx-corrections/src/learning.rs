#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedCorrection {
    pub source: String,
    pub replacement: String,
}

pub fn learn_correction(
    source: impl AsRef<str>,
    replacement: impl AsRef<str>,
    insertion_succeeded: bool,
) -> Option<LearnedCorrection> {
    if !insertion_succeeded {
        return None;
    }

    let source = source.as_ref().trim();
    let replacement = replacement.as_ref().trim();
    if source.is_empty() || replacement.is_empty() || source == replacement {
        return None;
    }

    // Only learn phrase-level normalization when the underlying words stay the same.
    // Broader rewrites stay out of the automatic path until Pepper X has explicit review.
    if normalized_tokens(source) != normalized_tokens(replacement) {
        return None;
    }

    Some(LearnedCorrection {
        source: source.into(),
        replacement: replacement.into(),
    })
}

fn normalized_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|token| {
            let normalized: String = token
                .chars()
                .filter(|character| character.is_alphanumeric())
                .flat_map(|character| character.to_lowercase())
                .collect();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect()
}
