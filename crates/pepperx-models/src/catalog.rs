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
}

const SUPPORTED_MODELS: [CatalogModel; 2] = [
    CatalogModel {
        id: "nemo-parakeet-tdt-0.6b-v2-int8",
        kind: ModelKind::Asr,
        install_path: "asr/nemo-parakeet-tdt-0.6b-v2-int8",
        required_files: &[
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ],
        install_layout: InstallLayout::Directory,
        download_artifact: DownloadArtifact {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2",
            file_name: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2",
            kind: DownloadArtifactKind::TarBz2,
            strip_prefix: Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8"),
        },
    },
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
    },
];

pub fn supported_models() -> &'static [CatalogModel] {
    &SUPPORTED_MODELS
}

pub fn catalog_model(id: &str) -> Option<&'static CatalogModel> {
    supported_models().iter().find(|model| model.id == id)
}

pub fn default_model(kind: ModelKind) -> &'static CatalogModel {
    supported_models()
        .iter()
        .find(|model| model.kind == kind)
        .expect("Pepper X catalog must include a default model for each kind")
}
