// src/position_reader.rs
//! 角色坐标读取模块 - 通过模板匹配识别小地图下方的"危险 X,Y"坐标数字
//!
//! 💡 为什么要读游戏内坐标,而不是继续用画面像素差异去猜:
//! - 之前用"画面有没有变化"来判断角色动没动,容易被角色待机动画、
//!   环境特效(火把、粒子)干扰,而且没法区分"画面变了多少对应实际
//!   走了多远"。
//! - 游戏 UI 本身就在小地图下方实时显示了坐标数字,直接读这个数字,
//!   准确、直接,还能顺便知道"走了多远"而不只是"动没动"。
//!
//! 识别方式:跟怪物名字识别(monster_matcher.rs)是同一套思路 —— 不用
//! 通用 OCR,而是给 0~9 十个数字 + 逗号建立模板库,对固定位置的坐标
//! 显示区域做连通域分割(每个数字字符是一个连通块),按从左到右的顺序
//! 逐个字符做模板匹配,拼出完整的坐标字符串再解析成两个整数。
//!
//! 📋 使用流程(跟建怪物名字模板库是一样的套路):
//! 1. 先用 `debug_dump_position_roi` 把坐标区域整块裁出来存盘,肉眼确认
//!    ROI 范围是不是刚好框住那串"危险 214,179"文字(不多不少)。
//! 2. 用 `debug_crop_digit_boxes` 把这块区域里检测到的每个数字字符
//!    单独裁剪存盘。
//! 3. 挑出 0~9 十个数字各一张、逗号一张,分别命名成 `0.png`...`9.png`、
//!    `comma.png`,放进 `assets/digits/` 目录，并在 `EMBEDDED_DIGIT_TEMPLATES`
//!    里补一行对应的 `include_bytes!` 条目（模板图编译时就嵌进了可执行
//!    文件，不是运行时读目录，所以新增模板需要改这个数组）。
//! 4. 调用 `load_digit_templates()` 加载,再调用 `read_position()`
//!    读取当前坐标。

use opencv::{
    Result,
    core::{self, Mat, Point, Rect, Scalar, Size, Vector, min_max_loc},
    imgcodecs::{self, IMREAD_COLOR},
    imgproc::{self, TemplateMatchModes, match_template},
    prelude::*,
};
pub const TEMPLATE_REFERENCE_PHYSICAL_WIDTH: f64 = 3000.0;
pub const TEMPLATE_REFERENCE_PHYSICAL_HEIGHT: f64 = 1716.0;

/// 一个数字/符号的参考模板小图。label 是 "0".."9" 或 ","。
#[derive(Debug, Clone)]
pub struct DigitTemplate {
    pub label: String,
    pub template: Mat,
}

/// 坐标读取的检测参数。ROI 是相对"游戏画面"(不含标题栏)的比例,
/// 跟 monster_detector::DetectorConfig 是同一套设计思路。
#[derive(Debug, Clone)]
pub struct PositionReaderConfig {
    pub roi_left_frac: f64,
    pub roi_top_frac: f64,
    pub roi_right_frac: f64,
    pub roi_bottom_frac: f64,

    pub s_max: f64,
    pub v_min: f64,

    pub close_kernel_w: i32,
    pub close_kernel_h: i32,

    pub min_h: i32,
    pub max_h: i32,
    pub min_w: i32,
    pub max_w: i32,
    pub min_area: i32,
}

impl Default for PositionReaderConfig {
    fn default() -> Self {
        Self {
            roi_left_frac: 0.83,
            roi_top_frac: 0.25,
            roi_right_frac: 0.99,
            roi_bottom_frac: 0.30,

            s_max: 60.0,
            v_min: 150.0,

            close_kernel_w: 1,
            close_kernel_h: 3,

            min_h: 3,
            max_h: 30,
            min_w: 1,
            max_w: 40,
            min_area: 3,
        }
    }
}

impl PositionReaderConfig {
    pub fn scaled_for(&self, scale_x: f64, scale_y: f64) -> PositionReaderConfig {
        let sx = scale_x.max(0.05);
        let sy = scale_y.max(0.05);
        let scale_area = sx * sy;

        let scale_i32 = |v: i32, s: f64| ((v as f64) * s).round().max(1.0) as i32;

        PositionReaderConfig {
            close_kernel_w: scale_i32(self.close_kernel_w, sx),
            close_kernel_h: scale_i32(self.close_kernel_h, sy),
            min_h: scale_i32(self.min_h, sy),
            max_h: scale_i32(self.max_h, sy),
            min_w: scale_i32(self.min_w, sx),
            max_w: scale_i32(self.max_w, sx),
            min_area: ((self.min_area as f64) * scale_area).round().max(1.0) as i32,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CharBox {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl CharBox {
    fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

const EMBEDDED_DIGIT_TEMPLATES: &[(&str, &[u8])] = &[
    ("0", include_bytes!("../../assets/digits/0.png")),
    ("1", include_bytes!("../../assets/digits/1.png")),
    ("2", include_bytes!("../../assets/digits/2.png")),
    ("3", include_bytes!("../../assets/digits/3.png")),
    ("4", include_bytes!("../../assets/digits/4.png")),
    ("5", include_bytes!("../../assets/digits/5.png")),
    ("6", include_bytes!("../../assets/digits/6.png")),
    ("7", include_bytes!("../../assets/digits/7.png")),
    ("8", include_bytes!("../../assets/digits/8.png")),
    ("9", include_bytes!("../../assets/digits/9.png")),
    (",", include_bytes!("../../assets/digits/comma.png")),
];

pub fn load_digit_templates() -> Result<Vec<DigitTemplate>> {
    let mut templates = Vec::with_capacity(EMBEDDED_DIGIT_TEMPLATES.len());

    for (label, bytes) in EMBEDDED_DIGIT_TEMPLATES {
        let buf = Vector::from_slice(bytes);
        let template = imgcodecs::imdecode(&buf, IMREAD_COLOR)?;
        if template.empty() {
            println!("   ⚠️ [坐标数字模板库] 内置模板解码失败: '{}'", label);
            continue;
        }

        println!("   📎 [坐标数字模板库] 已加载内置模板: '{}'", label);
        templates.push(DigitTemplate {
            label: label.to_string(),
            template,
        });
    }

    println!(
        "   ✅ [坐标数字模板库] 共加载 {} 个数字/符号模板",
        templates.len()
    );

    Ok(templates)
}

fn compute_roi_rect(frame_w: i32, frame_h: i32, cfg: &PositionReaderConfig) -> Rect {
    let x = (frame_w as f64 * cfg.roi_left_frac) as i32;
    let y = (frame_h as f64 * cfg.roi_top_frac) as i32;
    let w = (frame_w as f64 * (cfg.roi_right_frac - cfg.roi_left_frac)) as i32;
    let h = (frame_h as f64 * (cfg.roi_bottom_frac - cfg.roi_top_frac)) as i32;
    Rect::new(x, y, w.max(1), h.max(1))
}

fn detect_char_boxes(roi_bgr: &Mat, cfg: &PositionReaderConfig) -> Result<Vec<CharBox>> {
    let mut hsv = Mat::default();
    imgproc::cvt_color(
        roi_bgr,
        &mut hsv,
        imgproc::COLOR_BGR2HSV,
        0,
        core::AlgorithmHint::ALGO_HINT_DEFAULT,
    )?;

    let lower = Scalar::new(0.0, 0.0, cfg.v_min, 0.0);
    let upper = Scalar::new(180.0, cfg.s_max, 255.0, 0.0);
    let mut mask = Mat::default();
    core::in_range(&hsv, &lower, &upper, &mut mask)?;

    let kernel = imgproc::get_structuring_element(
        imgproc::MORPH_RECT,
        Size::new(cfg.close_kernel_w, cfg.close_kernel_h),
        Point::new(-1, -1),
    )?;
    let mut closed = Mat::default();
    imgproc::morphology_ex(
        &mask,
        &mut closed,
        imgproc::MORPH_CLOSE,
        &kernel,
        Point::new(-1, -1),
        1,
        core::BORDER_CONSTANT,
        imgproc::morphology_default_border_value()?,
    )?;

    let mut labels = Mat::default();
    let mut stats = Mat::default();
    let mut centroids = Mat::default();
    let num = imgproc::connected_components_with_stats(
        &closed,
        &mut labels,
        &mut stats,
        &mut centroids,
        8,
        core::CV_32S,
    )?;

    let mut boxes = Vec::new();
    for i in 1..num {
        let x = *stats.at_2d::<i32>(i, imgproc::CC_STAT_LEFT)?;
        let y = *stats.at_2d::<i32>(i, imgproc::CC_STAT_TOP)?;
        let w = *stats.at_2d::<i32>(i, imgproc::CC_STAT_WIDTH)?;
        let h = *stats.at_2d::<i32>(i, imgproc::CC_STAT_HEIGHT)?;
        let area = *stats.at_2d::<i32>(i, imgproc::CC_STAT_AREA)?;

        if h > cfg.min_h && h < cfg.max_h && w > cfg.min_w && w < cfg.max_w && area > cfg.min_area {
            boxes.push(CharBox { x, y, w, h });
        }
    }

    boxes.sort_by_key(|b| b.x);

    Ok(boxes)
}

fn match_char(
    roi_bgr: &Mat,
    char_box: &CharBox,
    templates: &[DigitTemplate],
    min_confidence: f32,
    scale_x: f64,
    scale_y: f64,
) -> Result<Option<(String, f32)>> {
    const PADDING: i32 = 2;

    let img_w = roi_bgr.cols();
    let img_h = roi_bgr.rows();

    let x = (char_box.x - PADDING).max(0);
    let y = (char_box.y - PADDING).max(0);
    let w = (char_box.w + PADDING * 2).min(img_w - x);
    let h = (char_box.h + PADDING * 2).min(img_h - y);

    if w <= 0 || h <= 0 {
        return Ok(None);
    }

    let cropped = Mat::roi(roi_bgr, Rect::new(x, y, w, h))?.try_clone()?;

    let mut best_label: Option<String> = None;
    let mut best_score: f32 = 0.0;

    for tpl in templates {
        let need_resize = (scale_x - 1.0).abs() > 0.01 || (scale_y - 1.0).abs() > 0.01;
        let scaled_template = if need_resize {
            let new_w = ((tpl.template.cols() as f64) * scale_x).round().max(1.0) as i32;
            let new_h = ((tpl.template.rows() as f64) * scale_y).round().max(1.0) as i32;
            let mut resized = Mat::default();
            let avg_scale = (scale_x + scale_y) / 2.0;
            let interpolation = if avg_scale < 1.0 {
                imgproc::INTER_AREA
            } else {
                imgproc::INTER_LINEAR
            };
            match imgproc::resize(
                &tpl.template,
                &mut resized,
                core::Size::new(new_w, new_h),
                0.0,
                0.0,
                interpolation,
            ) {
                Ok(_) => resized,
                Err(_) => tpl.template.clone(),
            }
        } else {
            tpl.template.clone()
        };

        if scaled_template.cols() > cropped.cols() || scaled_template.rows() > cropped.rows() {
            continue;
        }

        let mut result = Mat::default();
        match_template(
            &cropped,
            &scaled_template,
            &mut result,
            TemplateMatchModes::TM_CCOEFF_NORMED.into(),
            &core::no_array(),
        )?;

        let mut min_val: f64 = 0.0;
        let mut max_val: f64 = 0.0;
        let mut min_loc = Point::default();
        let mut max_loc = Point::default();

        min_max_loc(
            &result,
            Some(&mut min_val),
            Some(&mut max_val),
            Some(&mut min_loc),
            Some(&mut max_loc),
            &core::no_array(),
        )?;

        let score = max_val as f32;
        if score > best_score {
            best_score = score;
            best_label = Some(tpl.label.clone());
        }
    }

    if best_score >= min_confidence {
        Ok(best_label.map(|l| (l, best_score)))
    } else {
        Ok(None)
    }
}

pub fn read_position(
    frame_bgr: &Mat,
    cfg: &PositionReaderConfig,
    templates: &[DigitTemplate],
    min_confidence: f32,
) -> Option<(i32, i32)> {
    if templates.is_empty() {
        return None;
    }

    let scale_x = frame_bgr.cols() as f64 / TEMPLATE_REFERENCE_PHYSICAL_WIDTH;
    let scale_y = frame_bgr.rows() as f64 / TEMPLATE_REFERENCE_PHYSICAL_HEIGHT;
    let scaled_cfg = cfg.scaled_for(scale_x, scale_y);

    let roi_rect = compute_roi_rect(frame_bgr.cols(), frame_bgr.rows(), cfg);
    let roi_img = match Mat::roi(frame_bgr, roi_rect).and_then(|r| r.try_clone()) {
        Ok(r) => r,
        Err(_) => return None,
    };

    let boxes = match detect_char_boxes(&roi_img, &scaled_cfg) {
        Ok(b) => b,
        Err(_) => return None,
    };

    if boxes.is_empty() {
        return None;
    }

    let mut chars = String::new();
    for b in &boxes {
        if let Ok(Some((label, _score))) =
            match_char(&roi_img, b, templates, min_confidence, scale_x, scale_y)
        {
            chars.push_str(&label);
        }
    }

    // 🐛 之前这里有个 bug：拆出超过 2 段(比如误识别出多余的逗号)时，
    // 会尝试"硬凑"出一个坐标——但凑的逻辑是错的，会把中间某一段数字
    // 拼成一个跟真实坐标完全不沾边的错误值(实测抓到过真实案例：一帧
    // 正常坐标被拆成三段，硬凑出一个和上一帧相差 204 个单位的离谱坐标，
    // 这个坏值被标定逻辑当成"真实移动"吃了进去，把整个移动矩阵都
    // 带偏了)。
    //
    // 正确做法：拆出来不是正好 2 段，就是这一帧没读对，老老实实返回
    // None，让调用方(writer.rs)用回上一帧的旧坐标兜底，好过硬凑一个
    // 可能错得离谱的"坐标"出来污染下游(标定/寻路)的计算。
    let parts: Vec<&str> = chars.split(',').collect();
    if parts.len() != 2 {
        return None;
    }

    let x: i32 = parts[0].trim().parse().ok()?;
    let y: i32 = parts[1].trim().parse().ok()?;

    Some((x, y))
}

pub fn debug_dump_position_roi(
    frame_bgr: &Mat,
    cfg: &PositionReaderConfig,
    out_path: &str,
) -> Result<()> {
    let roi_rect = compute_roi_rect(frame_bgr.cols(), frame_bgr.rows(), cfg);
    let roi_img = Mat::roi(frame_bgr, roi_rect)?.try_clone()?;
    let params = Vector::new();
    imgcodecs::imwrite(out_path, &roi_img, &params)?;
    Ok(())
}

pub fn debug_crop_digit_boxes(
    frame_bgr: &Mat,
    cfg: &PositionReaderConfig,
    out_dir: &str,
) -> Result<()> {
    let scale_x = frame_bgr.cols() as f64 / TEMPLATE_REFERENCE_PHYSICAL_WIDTH;
    let scale_y = frame_bgr.rows() as f64 / TEMPLATE_REFERENCE_PHYSICAL_HEIGHT;
    let scaled_cfg = cfg.scaled_for(scale_x, scale_y);

    let roi_rect = compute_roi_rect(frame_bgr.cols(), frame_bgr.rows(), cfg);
    let roi_img = Mat::roi(frame_bgr, roi_rect)?.try_clone()?;
    let boxes = detect_char_boxes(&roi_img, &scaled_cfg)?;

    std::fs::create_dir_all(out_dir).ok();

    for (i, b) in boxes.iter().enumerate() {
        let cropped = Mat::roi(&roi_img, b.rect())?.try_clone()?;
        let out_path = format!("{}/char_{:02}_{}_{}.png", out_dir, i, b.x, b.y);
        let params = Vector::new();
        imgcodecs::imwrite(&out_path, &cropped, &params)?;
    }

    println!(
        "   🗂️ [调试] 已把坐标区域里 {} 个候选字符单独裁剪存到目录: {}",
        boxes.len(),
        out_dir
    );

    Ok(())
}
