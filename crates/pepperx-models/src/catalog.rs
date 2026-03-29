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
pub struct CatalogModel {
    pub id: &'static str,
    pub kind: ModelKind,
    pub install_path: &'static str,
    pub required_files: &'static [&'static str],
    pub install_layout: InstallLayout,
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
    },
    CatalogModel {
        id: "qwen2.5-3b-instruct-q4_k_m.gguf",
        kind: ModelKind::Cleanup,
        install_path: "cleanup/qwen2.5-3b-instruct-q4_k_m.gguf",
        required_files: &[],
        install_layout: InstallLayout::File,
    },
];

pub fn supported_models() -> &'static [CatalogModel] {
    &SUPPORTED_MODELS
}
