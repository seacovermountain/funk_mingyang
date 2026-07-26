pub fn capture_window(window: &Window) -> Option<(Vec<u8>, u32, u32)> {
    let xcap_image = window.capture_image().ok()?;
    let width = xcap_image.width();
    let height = xcap_image.height();

    // 将 xcap 内部的图片对象转换成标准的通用 Vec<u8> 原始像素数据
    let raw_pixels = xcap_image.into_raw();

    Some((raw_pixels, width, height))
}
