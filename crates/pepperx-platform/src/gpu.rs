use std::sync::OnceLock;

static GPU_INFO: OnceLock<GpuInfo> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuInfo {
    pub available: bool,
    pub vendor: Option<&'static str>,
    pub recommended_layers: u32,
}

pub fn get_gpu_info() -> GpuInfo {
    *GPU_INFO.get_or_init(|| {
        if cfg!(target_os = "linux") {
            if std::path::Path::new("/proc/driver/nvidia/version").exists() {
                return GpuInfo {
                    available: true,
                    vendor: Some("NVIDIA"),
                    recommended_layers: 999,
                };
            }
            if std::path::Path::new("/dev/dri").exists() {
                return GpuInfo {
                    available: true,
                    vendor: Some("AMD/Intel"),
                    recommended_layers: 80,
                };
            }
        }
        GpuInfo {
            available: false,
            vendor: None,
            recommended_layers: 0,
        }
    })
}