// src/game/button_finder.rs
//
// 职责：编译时嵌入每个按钮的模板图（见 EMBEDDED_BUTTONS）-> 在当前帧
// 里做模板匹配。原来是读 buttons.toml + 同目录图片文件，现在图片和
// 元数据都嵌进了可执行文件，不再读外部文件。
//
// 分辨率处理思路不变：模板按 scale = 当前窗口宽度/基准窗口宽度 缩放后
// 再去匹配整帧画面，窗口宽度没怎么变时复用上次缩放好的模板。
//
// 点击位置 = 匹配区域中心 + (click_dx, click_dy) * scale。
// 大多数按钮偏移量是 (0,0)；minimap 这种匹配锚点和点击点不重合的，
// 偏移量在 EMBEDDED_BUTTONS 里单独配置。

use opencv::core::{AlgorithmHint, Mat, Point, Size, Vector};
use opencv::imgcodecs;
use opencv::imgproc;
use opencv::prelude::*;
use std::sync::Mutex;
use std::time::Instant;

use crate::game::state::ButtonInfo;

/// 这批按钮模板图对应的基准窗口宽度，运行时按当前窗口实际宽度换算
/// 缩放系数（原来在 buttons.toml 里的 base_window_width，现在图片和
/// 配置都嵌进可执行文件了，这个值也一起挪过来）。
const BASE_WINDOW_WIDTH: u32 = 3264;

/// 一个内置按钮模板：名字、编译时嵌入的图片字节、点击偏移量。
/// 原来是从 buttons.toml + 同目录下的图片文件读的，现在图片和这几个
/// 元数据都在编译时嵌入可执行文件——按钮图标基本不会变，嵌进去以后
/// 部署只需要一个可执行文件，不用再带 assets/buttons/ 这个文件夹。
struct EmbeddedButton {
    name: &'static str,
    bytes: &'static [u8],
    click_dx: i32,
    click_dy: i32,
}

const EMBEDDED_BUTTONS: &[EmbeddedButton] = &[
    EmbeddedButton {
        name: "pickup",
        bytes: include_bytes!("../../assets/buttons/pickup.png"),
        click_dx: 0,
        click_dy: 0,
    },
    EmbeddedButton {
        name: "attack",
        bytes: include_bytes!("../../assets/buttons/attack.png"),
        click_dx: 0,
        click_dy: 0,
    },
    EmbeddedButton {
        name: "close",
        bytes: include_bytes!("../../assets/buttons/close.png"),
        click_dx: 0,
        click_dy: 0,
    },
    // minimap 比较特殊：模板是圆盘顶部固定不变的 "N" 徽章，
    // 但真正要点击的是徽章下方的圆盘本体，所以点击位置需要在匹配到的
    // 位置上再往下偏移一段距离（click_dy），而不是直接点匹配到的地方。
    EmbeddedButton {
        name: "minimap",
        bytes: include_bytes!("../../assets/buttons/minimap.png"),
        click_dx: 0,
        click_dy: 132,
    },
];

struct Template {
    name: String,
    mat: Mat,
    click_dx: i32,
    click_dy: i32,
}

impl Template {
    fn try_clone(&self) -> opencv::Result<Template> {
        Ok(Template {
            name: self.name.clone(),
            mat: self.mat.try_clone()?,
            click_dx: self.click_dx,
            click_dy: self.click_dy,
        })
    }
}

pub struct ButtonFinder {
    templates: Vec<Template>,
    base_window_width: u32,
    resized_cache: Mutex<Option<(f64, Vec<Template>)>>,
}

impl ButtonFinder {
    /// 从编译时嵌入的按钮图标数据加载全部按钮模板（不再读外部文件）。
    pub fn load() -> Result<Self, String> {
        let mut templates = Vec::with_capacity(EMBEDDED_BUTTONS.len());
        for entry in EMBEDDED_BUTTONS {
            let buf = Vector::from_slice(entry.bytes);
            let mat = imgcodecs::imdecode(&buf, imgcodecs::IMREAD_GRAYSCALE)
                .map_err(|e| format!("解码内置按钮模板失败 {}: {}", entry.name, e))?;

            if mat.empty() {
                return Err(format!("内置按钮模板是空图: {}", entry.name));
            }

            println!("🖼️  已加载内置按钮模板: {}", entry.name);
            templates.push(Template {
                name: entry.name.to_string(),
                mat,
                click_dx: entry.click_dx,
                click_dy: entry.click_dy,
            });
        }

        println!(
            "✅ 按钮模板加载完成，共 {} 个（基准窗口宽度 {}）",
            templates.len(),
            BASE_WINDOW_WIDTH
        );

        Ok(ButtonFinder {
            templates,
            base_window_width: BASE_WINDOW_WIDTH,
            resized_cache: Mutex::new(None),
        })
    }

    fn scaled_templates(&self, current_window_width: u32) -> opencv::Result<(f64, Vec<Template>)> {
        let scale = current_window_width as f64 / self.base_window_width as f64;

        {
            let cache = self.resized_cache.lock().unwrap();
            if let Some((cached_scale, cached)) = cache.as_ref() {
                if (cached_scale - scale).abs() < 0.01 {
                    let cloned: opencv::Result<Vec<Template>> =
                        cached.iter().map(Template::try_clone).collect();
                    return Ok((scale, cloned?));
                }
            }
        }

        let mut resized = Vec::with_capacity(self.templates.len());
        for t in &self.templates {
            let mut dst = Mat::default();
            imgproc::resize(
                &t.mat,
                &mut dst,
                Size::new(0, 0),
                scale,
                scale,
                imgproc::INTER_LINEAR,
            )?;
            resized.push(Template {
                name: t.name.clone(),
                mat: dst,
                click_dx: t.click_dx,
                click_dy: t.click_dy,
            });
        }

        let snapshot: opencv::Result<Vec<Template>> =
            resized.iter().map(Template::try_clone).collect();
        *self.resized_cache.lock().unwrap() = Some((scale, resized));
        Ok((scale, snapshot?))
    }

    /// 在当前帧（灰度图）里找所有按钮，返回真实窗口坐标下的识别 + 点击位置。
    pub fn find_buttons(
        &self,
        frame_gray: &Mat,
        current_window_width: u32,
        threshold: f64,
    ) -> opencv::Result<Vec<ButtonInfo>> {
        let (scale, templates) = self.scaled_templates(current_window_width)?;
        let mut found = Vec::new();

        for t in &templates {
            if t.mat.cols() > frame_gray.cols() || t.mat.rows() > frame_gray.rows() {
                continue;
            }

            let mut result = Mat::default();
            imgproc::match_template(
                frame_gray,
                &t.mat,
                &mut result,
                imgproc::TM_CCOEFF_NORMED,
                &Mat::default(),
            )?;

            let mut min_val = 0f64;
            let mut max_val = 0f64;
            let mut min_loc = Point::default();
            let mut max_loc = Point::default();
            opencv::core::min_max_loc(
                &result,
                Some(&mut min_val),
                Some(&mut max_val),
                Some(&mut min_loc),
                Some(&mut max_loc),
                &Mat::default(),
            )?;

            if max_val >= threshold {
                let width = t.mat.cols() as u32;
                let height = t.mat.rows() as u32;

                let center_x = max_loc.x + width as i32 / 2;
                let center_y = max_loc.y + height as i32 / 2;
                let click_x = center_x + (t.click_dx as f64 * scale).round() as i32;
                let click_y = center_y + (t.click_dy as f64 * scale).round() as i32;

                found.push(ButtonInfo {
                    name: t.name.clone(),
                    x: max_loc.x,
                    y: max_loc.y,
                    width,
                    height,
                    confidence: max_val,
                    click_x,
                    click_y,
                    updated_at: Instant::now(),
                });
            }
        }

        Ok(found)
    }
}

/// 把 xcap 截图拿到的 RGBA 原始像素转换成 OpenCV 灰度图 Mat。
pub fn rgba_to_gray_mat(raw: &[u8], width: u32, height: u32) -> opencv::Result<Mat> {
    let borrowed = Mat::new_rows_cols_with_data(height as i32, (width * 4) as i32, raw)?;
    let mat_1ch = borrowed.try_clone()?;
    let mat_rgba = mat_1ch.reshape(4, height as i32)?;

    let mut gray = Mat::default();
    imgproc::cvt_color(
        &mat_rgba,
        &mut gray,
        imgproc::COLOR_RGBA2GRAY,
        0,
        AlgorithmHint::ALGO_HINT_DEFAULT,
    )?;
    Ok(gray)
}
