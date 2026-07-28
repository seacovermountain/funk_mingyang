use xcap::Window;

use std::path::{Path, PathBuf};

/// 截取指定窗口画面，返回 (RGBA 原始像素, 宽, 高)。
///
/// 失败情况（如窗口已关闭、权限不足）统一返回 `None`，
/// 由调用方决定是重试还是跳过本轮。
pub fn capture_window(window: &Window) -> Option<(Vec<u8>, u32, u32)> {
    let xcap_image = window.capture_image().ok()?;
    let width = xcap_image.width();
    let height = xcap_image.height();

    // 将 xcap 内部的图片对象转换成标准的通用 Vec<u8> 原始像素数据
    let raw_pixels = xcap_image.into_raw();

    Some((raw_pixels, width, height))
}

/// 可执行文件自己所在的目录。所有资源路径都应该以这个目录为基准拼接，
/// 不依赖"当前工作目录"——双击运行、拖到桌面运行、从任意路径运行，
/// 只要 assets/、models/、config.toml 跟可执行文件放在一起，就能找到。
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .expect("❌ 获取可执行文件自身路径失败")
        .parent()
        .expect("❌ 获取可执行文件所在目录失败")
        .to_path_buf()
}

/// 把相对路径拼到可执行文件所在目录下，直接返回 PathBuf——各个
/// ::load() 都接受 impl AsRef<Path>，不需要再转成 String。
pub fn resource_path(base: &Path, relative: &str) -> PathBuf {
    base.join(relative)
}
