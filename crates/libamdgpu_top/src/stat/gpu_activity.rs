use std::path::PathBuf;
use crate::AMDGPU::{DeviceHandle, FAMILY_NAME, GpuMetrics, MetricsInfo};

#[derive(Debug, Clone)]
pub struct GpuActivity {
    pub gfx: Option<u16>, // %
    pub umc: Option<u16>, // %
    pub media: Option<u16>, // %
}

impl GpuActivity {
    pub fn get<P: Into<PathBuf>>(
        amdgpu_dev: &DeviceHandle,
        sysfs_path: P,
        family_name: FAMILY_NAME,
    ) -> Option<Self> {
        let path = sysfs_path.into();

        if let Ok(metrics) = amdgpu_dev.get_gpu_metrics_from_sysfs_path(&path) {
            Some(Self::from_gpu_metrics(&metrics))
        } else {
            // Some Raven/Picasso/Raven2 APU always report gpu_busy_percent as 100.
            // ref: https://gitlab.freedesktop.org/drm/amd/-/issues/1932
            // gpu_metrics is supported from Renoir APU.
            if let FAMILY_NAME::RV = family_name {
                None
            } else {
                Some(Self::get_from_sysfs(&path))
            }
        }
    }

    pub fn from_gpu_metrics(metrics: &GpuMetrics) -> Self {
        let Some(header) = metrics.get_header() else {
            return Self { gfx: None, umc: None, media: None }
        };

        let [gfx, umc, media] = [
            metrics.get_average_gfx_activity(),
            metrics.get_average_umc_activity(),
            metrics.get_average_mm_activity(),
        ].map(|activity| activity.map(|v| {
            if header.format_revision == 2 {
                /* for APU (gpu_metrics v2.x) */
                v.saturating_div(100)
            } else {
                v
            }
        }));

        Self { gfx, umc, media }
    }

    pub fn get_from_sysfs<P: Into<PathBuf>>(sysfs_path: P) -> Self {
        let path = sysfs_path.into();
        let [gfx, umc] = ["gpu_busy_percent", "mem_busy_percent"].map(|name| {
            std::fs::read_to_string(&path.join(name)).ok()
                .and_then(|s| s.trim_end().parse().ok())
        });


        Self { gfx, umc, media: None }
    }
}
