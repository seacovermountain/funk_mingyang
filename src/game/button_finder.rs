// src/game/button_finder.rs
//
// 职责：读 buttons.toml -> 加载每个按钮的模板图 -> 在当前帧里做模板匹配。
//
// 分辨率处理思路不变：模板按 scale = 当前窗口宽度/基准窗口宽度 缩放后
// 再去匹配整帧画面，窗口宽度没怎么变时复用上次缩放好的模板。
//
// 点击位置 = 匹配区域中心 + (click_dx, click_dy) * scale。
// 大多数按钮偏移量是 (0,0)；minimap 这种匹配锚点和点击点不重合的，
// 偏移量从 buttons.toml 里读。

use opencv::core::{AlgorithmHint, Mat, Point, Size};
use opencv::imgcodecs;
use opencv::imgproc;
use opencv::prelude::*;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use crate::game::state::ButtonInfo;

#[derive(Debug, Deserialize)]
struct ButtonsFile {
    base_window_width: u32,
    buttons: Vec<ButtonEntry>,
}

#[derive(Debug, Deserialize)]
struct ButtonEntry {
    name: String,
    template: String,
    click_dx: i32,
    click_dy: i32,
}

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
    /// 从 buttons.toml 加载全部按钮模板。
    /// `config_path` 例如 "assets/buttons/buttons.toml"；
    /// 模板图路径按这个配置文件所在目录解析。
    pub fn load(config_path: &str) -> Result<Self, String> {
        let config_path = Path::new(config_path);
        let dir = config_path.parent().unwrap_or_else(|| Path::new("."));

        let raw = fs::read_to_string(config_path)
            .map_err(|e| format!("读取按钮配置失败 {:?}: {}", config_path, e))?;

        let parsed: ButtonsFile = toml::from_str(&raw)
            .map_err(|e| format!("解析按钮配置失败 {:?}: {}", config_path, e))?;

        let mut templates = Vec::with_capacity(parsed.buttons.len());
        for entry in &parsed.buttons {
            let img_path = dir.join(&entry.template);
            let mat = imgcodecs::imread(
                img_path.to_str().unwrap_or_default(),
                imgcodecs::IMREAD_GRAYSCALE,
            )
            .map_err(|e| format!("加载按钮模板失败 {:?}: {}", img_path, e))?;

            if mat.empty() {
                return Err(format!("按钮模板是空图: {:?}", img_path));
            }

            println!("🖼️  已加载按钮模板: {} ({:?})", entry.name, img_path);
            templates.push(Template {
                name: entry.name.clone(),
                mat,
                click_dx: entry.click_dx,
                click_dy: entry.click_dy,
            });
        }

        println!(
            "✅ 按钮模板加载完成，共 {} 个（基准窗口宽度 {}）",
            templates.len(),
            parsed.base_window_width
        );

        Ok(ButtonFinder {
            templates,
            base_window_width: parsed.base_window_width,
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
