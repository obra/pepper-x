#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSupport {
    pub supported: bool,
}

pub fn download_support() -> DownloadSupport {
    DownloadSupport { supported: false }
}
