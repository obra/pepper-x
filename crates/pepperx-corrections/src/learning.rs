#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedCorrection {
    pub source: String,
    pub replacement: String,
}

pub fn learn_correction(
    source: impl Into<String>,
    replacement: impl Into<String>,
) -> LearnedCorrection {
    LearnedCorrection {
        source: source.into(),
        replacement: replacement.into(),
    }
}
