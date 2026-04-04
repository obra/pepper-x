#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Asr,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallLayout {
    Directory,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadArtifactKind {
    File,
    TarBz2,
    /// Download multiple individual files into a directory.
    MultiFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadArtifact {
    pub url: &'static str,
    pub file_name: &'static str,
    pub kind: DownloadArtifactKind,
    pub strip_prefix: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogModel {
    pub id: &'static str,
    pub kind: ModelKind,
    pub install_path: &'static str,
    pub required_files: &'static [&'static str],
    pub install_layout: InstallLayout,
    pub download_artifact: DownloadArtifact,
    /// For MultiFile downloads: the individual file URLs to fetch.  Each entry
    /// is `(relative_file_name, url)`.  Empty for non-MultiFile artifacts.
    pub download_files: &'static [(&'static str, &'static str)],
    /// Chat template family for cleanup models (e.g. "chatml", "gemma").
    /// Determines how the system/user/assistant prompt is formatted.
    pub chat_template: &'static str,
}

const SUPPORTED_MODELS: [CatalogModel; 7] = [
    // Default ASR: Nemotron streaming int8 (parakeet-rs)
    CatalogModel {
        id: "nemotron-speech-streaming-en-0.6b",
        kind: ModelKind::Asr,
        install_path: "asr/nemotron-speech-streaming-en-0.6b",
        required_files: &[
            "encoder.onnx",
            "decoder_joint.onnx",
            "tokenizer.model",
        ],
        install_layout: InstallLayout::Directory,
        download_artifact: DownloadArtifact {
            url: "https://huggingface.co/smcleod/nemotron-speech-streaming-en-0.6b-int8/resolve/main/",
            file_name: "",
            kind: DownloadArtifactKind::MultiFile,
            strip_prefix: None,
        },
        download_files: &[
            ("encoder.onnx", "https://huggingface.co/smcleod/nemotron-speech-streaming-en-0.6b-int8/resolve/main/encoder.onnx"),
            ("decoder_joint.onnx", "https://huggingface.co/smcleod/nemotron-speech-streaming-en-0.6b-int8/resolve/main/decoder_joint.onnx"),
            ("tokenizer.model", "https://huggingface.co/smcleod/nemotron-speech-streaming-en-0.6b-int8/resolve/main/tokenizer.model"),
        ],
        chat_template: "",
    },
    // Legacy ASR: Parakeet TDT v3 (sherpa-onnx, kept for backwards compat)
    CatalogModel {
        id: "nemo-parakeet-tdt-0.6b-v3-int8",
        kind: ModelKind::Asr,
        install_path: "asr/nemo-parakeet-tdt-0.6b-v3-int8",
        required_files: &[
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ],
        install_layout: InstallLayout::Directory,
        download_artifact: DownloadArtifact {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
            file_name: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
            kind: DownloadArtifactKind::TarBz2,
            strip_prefix: Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"),
        },
        download_files: &[],
        chat_template: "",
    },
    // Default cleanup: Qwen 3.5 2B (requires llama-cpp-4)
    CatalogModel {
        id: "qwen3.5-2b-q4_k_m.gguf",
        kind: ModelKind::Cleanup,
        install_path: "cleanup/Qwen3.5-2B-Q4_K_M.gguf",
        required_files: &[],
        install_layout: InstallLayout::File,
        download_artifact: DownloadArtifact {
            url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf",
            file_name: "Qwen3.5-2B-Q4_K_M.gguf",
            kind: DownloadArtifactKind::File,
            strip_prefix: None,
        },
        download_files: &[],
        chat_template: "chatml",
    },
    // Fast cleanup: Qwen 3.5 0.8B (requires llama-cpp-4)
    CatalogModel {
        id: "qwen3.5-0.8b-q4_k_m.gguf",
        kind: ModelKind::Cleanup,
        install_path: "cleanup/Qwen3.5-0.8B-Q4_K_M.gguf",
        required_files: &[],
        install_layout: InstallLayout::File,
        download_artifact: DownloadArtifact {
            url: "https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_K_M.gguf",
            file_name: "Qwen3.5-0.8B-Q4_K_M.gguf",
            kind: DownloadArtifactKind::File,
            strip_prefix: None,
        },
        download_files: &[],
        chat_template: "chatml",
    },
    // Legacy cleanup: Qwen 2.5 3B
    CatalogModel {
        id: "qwen2.5-3b-instruct-q4_k_m.gguf",
        kind: ModelKind::Cleanup,
        install_path: "cleanup/qwen2.5-3b-instruct-q4_k_m.gguf",
        required_files: &[],
        install_layout: InstallLayout::File,
        download_artifact: DownloadArtifact {
            url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf?download=true",
            file_name: "qwen2.5-3b-instruct-q4_k_m.gguf",
            kind: DownloadArtifactKind::File,
            strip_prefix: None,
        },
        download_files: &[],
        chat_template: "chatml",
    },
    // Gemma 4 E2B cleanup (benchmark)
    CatalogModel {
        id: "gemma-4-e2b-it-q4_k_m.gguf",
        kind: ModelKind::Cleanup,
        install_path: "cleanup/gemma-4-E2B-it-Q4_K_M.gguf",
        required_files: &[],
        install_layout: InstallLayout::File,
        download_artifact: DownloadArtifact {
            url: "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/main/gemma-4-E2B-it-Q4_K_M.gguf",
            file_name: "gemma-4-E2B-it-Q4_K_M.gguf",
            kind: DownloadArtifactKind::File,
            strip_prefix: None,
        },
        download_files: &[],
        chat_template: "gemma",
    },
    // Gemma 4 E2B cleanup, aggressive quant (benchmark)
    CatalogModel {
        id: "gemma-4-e2b-it-iq2_m.gguf",
        kind: ModelKind::Cleanup,
        install_path: "cleanup/gemma-4-E2B-it-UD-IQ2_M.gguf",
        required_files: &[],
        install_layout: InstallLayout::File,
        download_artifact: DownloadArtifact {
            url: "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/main/gemma-4-E2B-it-UD-IQ2_M.gguf",
            file_name: "gemma-4-E2B-it-UD-IQ2_M.gguf",
            kind: DownloadArtifactKind::File,
            strip_prefix: None,
        },
        download_files: &[],
        chat_template: "gemma",
    },
];

pub fn supported_models() -> &'static [CatalogModel] {
    &SUPPORTED_MODELS
}

pub fn catalog_model(id: &str) -> Option<&'static CatalogModel> {
    supported_models().iter().find(|model| model.id == id)
}

pub fn chat_template_for_model(id: &str) -> &'static str {
    catalog_model(id).map(|m| m.chat_template).unwrap_or("chatml")
}

pub fn default_model(kind: ModelKind) -> &'static CatalogModel {
    supported_models()
        .iter()
        .find(|model| model.kind == kind)
        .expect("Pepper X catalog must include a default model for each kind")
}
