// src/game/util.rs
//
// 通用工具函数：截图、以及"RGBA 原始像素 -> OpenCV Mat"的格式转换。
// 灰度图给按钮模板匹配用，BGR 图给 OCR 识别用，两边共用同一份转换
// 逻辑，集中放这里，button_finder / ocr 各自就只剩自己的识别算法。

use opencv::{core, imgproc, prelude::*};
use xcap::Window;

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

/// 把 xcap 截图拿到的 RGBA 原始像素转换成 OpenCV 灰度图 Mat（按钮模板
/// 匹配用）。
pub fn rgba_to_gray_mat(raw: &[u8], width: u32, height: u32) -> opencv::Result<core::Mat> {
    let mat_rgba = rgba_to_mat(raw, width, height)?;
    let mut gray = core::Mat::default();
    imgproc::cvt_color(
        &mat_rgba,
        &mut gray,
        imgproc::COLOR_RGBA2GRAY,
        0,
        core::AlgorithmHint::ALGO_HINT_DEFAULT,
    )?;
    Ok(gray)
}

/// 把 xcap 截图拿到的 RGBA 原始像素转换成 OpenCV BGR Mat（OCR 识别、
/// 坐标数字识别用）。
pub fn rgba_to_bgr_mat(raw: &[u8], width: u32, height: u32) -> opencv::Result<core::Mat> {
    let mat_rgba = rgba_to_mat(raw, width, height)?;
    let mut bgr = core::Mat::default();
    imgproc::cvt_color(
        &mat_rgba,
        &mut bgr,
        imgproc::COLOR_RGBA2BGR,
        0,
        core::AlgorithmHint::ALGO_HINT_DEFAULT,
    )?;
    Ok(bgr)
}

/// 两个转换函数共用的第一步：把裸像素数据包成 4 通道 RGBA Mat。
///
/// `reshape()` 返回的是一个借用视图（`BoxedRef<Mat>`），不是独立的
/// `Mat`——这里 `try_clone()` 一次，转成真正拥有数据的 `Mat`，方便
/// 调用方（rgba_to_gray_mat / rgba_to_bgr_mat）拿到手之后直接用。
fn rgba_to_mat(raw: &[u8], width: u32, height: u32) -> opencv::Result<core::Mat> {
    let borrowed = core::Mat::new_rows_cols_with_data(height as i32, (width * 4) as i32, raw)?;
    let mat_1ch = borrowed.try_clone()?;
    mat_1ch.reshape(4, height as i32)?.try_clone()
}
