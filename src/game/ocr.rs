// src/game/ocr.rs
//! 怪物名字 + 物品名字统一识别模块 - 基于 OCR。
//!
//! 一帧只跑一次 ocr-rs 的 recognize()：怪物白名单和物品白名单从同一份
//! 识别结果里各自筛选，不重复推理。白名单匹配用编辑距离做模糊匹配，
//! 这一步很便宜（字符串比较），不需要为了它再多跑一次 OCR。
//!
//! ROI 处理：不单独框"战斗区域"去猜怪物/物品可能出现的位置（这个框
//! 太容易调错），直接对整帧截图做 OCR，只把聊天框那一块涂黑（聊天记录
//! 内容量大、会刷屏，其他文字反正匹配不上白名单，天然会被过滤掉）。
//!
//! 地图名字走同一份 OCR 结果里的固定牌匾区域，不需要白名单匹配——
//! 识别出什么文字就是什么，单纯用来记录"现在在哪张图"。

use image::{DynamicImage, RgbImage};
use ocr_rs::OcrEngine;
use opencv::{
    core::{self, Mat},
    imgproc,
    prelude::*,
};
use std::collections::HashSet;
use std::error::Error;

/// 识别 / 匹配到的候选文字框位置（整帧截图坐标系）。
#[derive(Debug, Clone, Copy)]
pub struct TextBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub area: i32,
}

#[derive(Debug, Clone)]
pub struct TextOcrConfig {
    // 聊天框区域（整帧截图的比例），识别前会把这块区域涂黑，避免
    // 聊天记录里的大量文字干扰识别 / 拖慢速度。
    // 实测坐标（3264×1864 截图上量出来的）：左34.6%~右65.5%，
    // 上87.0%~下98.9%。
    pub chat_box_left_frac: f64,
    pub chat_box_top_frac: f64,
    pub chat_box_right_frac: f64,
    pub chat_box_bottom_frac: f64,

    // 编辑距离容错：识别结果跟白名单允许差几个字符也算命中。
    pub max_edit_distance: usize,

    // OCR 识别置信度门槛，低于这个分数的文字块直接跳过，不参与匹配。
    pub min_confidence: f32,

    // 地图名字牌匾区域（整帧截图的比例）。
    pub map_name_left_frac: f64,
    pub map_name_top_frac: f64,
    pub map_name_right_frac: f64,
    pub map_name_bottom_frac: f64,

    // 血量数字区域（整帧截图的比例），比如角色头像左边那串 "2130/2130"。
    pub hp_left_frac: f64,
    pub hp_top_frac: f64,
    pub hp_right_frac: f64,
    pub hp_bottom_frac: f64,
}

impl Default for TextOcrConfig {
    fn default() -> Self {
        Self {
            chat_box_left_frac: 0.346,
            chat_box_top_frac: 0.870,
            chat_box_right_frac: 0.655,
            chat_box_bottom_frac: 0.989,
            max_edit_distance: 1,
            min_confidence: 0.5,
            map_name_left_frac: 0.76,
            map_name_top_frac: 0.02,
            map_name_right_frac: 0.99,
            map_name_bottom_frac: 0.085,

            hp_left_frac: 0.23,
            hp_top_frac: 0.935,
            hp_right_frac: 0.27,
            hp_bottom_frac: 0.96,
        }
    }
}

/// 单个 OCR 识别到的原始文字块，坐标是整帧截图坐标系（对整帧做识别，
/// 没有再裁 ROI，不需要额外换算偏移）。
#[derive(Debug, Clone)]
pub struct RawTextBlock {
    pub text: String,
    pub confidence: f32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 把 OpenCV 的 BGR Mat 转换成 `image` crate 用的 DynamicImage，
/// 供 ocr-rs 的 recognize() 使用。假设 Mat 是连续内存。
fn mat_bgr_to_dynamic_image(mat: &Mat) -> Result<DynamicImage, Box<dyn Error>> {
    let width = mat.cols() as u32;
    let height = mat.rows() as u32;
    let data = mat.data_bytes()?;

    let expected_len = (width as usize) * (height as usize) * 3;
    if data.len() < expected_len {
        return Err("Mat 数据长度小于预期，可能不是连续内存".into());
    }

    let mut rgb_buf = vec![0u8; expected_len];
    for i in 0..(width as usize * height as usize) {
        // BGR -> RGB
        rgb_buf[i * 3] = data[i * 3 + 2];
        rgb_buf[i * 3 + 1] = data[i * 3 + 1];
        rgb_buf[i * 3 + 2] = data[i * 3];
    }

    let rgb_image =
        RgbImage::from_raw(width, height, rgb_buf).ok_or("无法从像素数据构建 RgbImage")?;
    Ok(DynamicImage::ImageRgb8(rgb_image))
}

/// 判断一个字符是不是中文（CJK 统一表意文字，覆盖绝大多数常用汉字范围）。
fn is_chinese_char(c: char) -> bool {
    let code = c as u32;
    (0x4E00..=0x9FFF).contains(&code)
        || (0x3400..=0x4DBF).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
}

/// 过滤掉识别结果里的非中文字符（数字/血量/等级/标点等）。
fn filter_chinese_only(text: &str) -> String {
    text.chars().filter(|c| is_chinese_char(*c)).collect()
}

/// 在一段（可能是多个名字粘连在一起的）文字里，用滑动窗口找出白名单里
/// 每个名字有没有作为"近似子串"出现过，返回命中的所有白名单名字（去重）。
fn find_whitelist_matches(
    text: &str,
    whitelist: &HashSet<String>,
    max_edit_distance: usize,
) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();

    for candidate in whitelist {
        let candidate_len = candidate.chars().count();
        if candidate_len == 0 {
            continue;
        }

        // 短名字（1~2个字）按固定容错太松，容易随便沾上一个字就误判命中，
        // 名字越短容错应该越严格：1~2个字要求完全精确匹配。
        let allowed_distance = if candidate_len <= 2 {
            0
        } else {
            max_edit_distance
        };

        let best_dist = if chars.len() < candidate_len {
            strsim::levenshtein(text, candidate)
        } else {
            let mut min_dist = usize::MAX;
            for start in 0..=(chars.len() - candidate_len) {
                let window: String = chars[start..start + candidate_len].iter().collect();
                let dist = strsim::levenshtein(&window, candidate);
                if dist < min_dist {
                    min_dist = dist;
                }
            }
            min_dist
        };

        if best_dist <= allowed_distance {
            found.push(candidate.clone());
        }
    }

    found
}

/// 从同一份 OCR 结果里挑出落在"地图名字牌匾"区域内的文字块，直接返回
/// 识别到的文字（不需要跟白名单模糊匹配）。如果区域内有多个文字块，
/// 取置信度最高的那一个。
pub fn match_map_name(
    blocks: &[RawTextBlock],
    frame_w: i32,
    frame_h: i32,
    cfg: &TextOcrConfig,
) -> Option<(String, f32)> {
    let roi_x1 = (frame_w as f64 * cfg.map_name_left_frac) as i32;
    let roi_y1 = (frame_h as f64 * cfg.map_name_top_frac) as i32;
    let roi_x2 = (frame_w as f64 * cfg.map_name_right_frac) as i32;
    let roi_y2 = (frame_h as f64 * cfg.map_name_bottom_frac) as i32;

    let mut best: Option<(String, f32)> = None;

    for block in blocks {
        if block.confidence < cfg.min_confidence {
            continue;
        }
        let cx = block.x + block.w / 2;
        let cy = block.y + block.h / 2;
        if cx < roi_x1 || cx > roi_x2 || cy < roi_y1 || cy > roi_y2 {
            continue;
        }

        let cleaned = filter_chinese_only(&block.text);
        if cleaned.is_empty() {
            continue;
        }

        let better = match &best {
            Some((_, score)) => block.confidence > *score,
            None => true,
        };
        if better {
            best = Some((cleaned, block.confidence));
        }
    }

    best
}

pub struct TextOcrRecognizer {
    engine: OcrEngine,
}

/// OCR 模型（检测/识别模型 + 字典）在编译时嵌入进可执行文件——这三个
/// 文件加起来 60MB+，二进制体积会明显变大，换来的是部署只需要一个
/// 可执行文件，不用再带 models/ 这个目录。这三个文件基本不会变
/// （除非要换模型版本），嵌进去没有 config.toml 那种"要手动改"的顾虑。
const DET_MODEL_BYTES: &[u8] = include_bytes!("../../models/PP-OCRv6_medium_det.mnn");
const REC_MODEL_BYTES: &[u8] = include_bytes!("../../models/PP-OCRv6_medium_rec.mnn");
const KEYS_BYTES: &[u8] = include_bytes!("../../models/ppocr_keys_v6_medium.txt");

impl TextOcrRecognizer {
    /// 从编译时嵌入的模型数据加载 OCR 引擎（不再读外部文件），
    /// 只在程序启动时初始化一次，怪物识别和物品识别共用这一个引擎实例。
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let engine = OcrEngine::from_bytes(DET_MODEL_BYTES, REC_MODEL_BYTES, KEYS_BYTES, None)?;
        Ok(Self { engine })
    }

    /// 一帧只识别一次：把聊天框区域涂黑，然后对整帧截图做 OCR，返回所有
    /// 识别到的原始文字块（未经白名单过滤）。怪物识别和物品识别都从这
    /// 一份结果里各自筛选，不需要分别再截图/再跑一次 OCR。
    pub fn recognize_frame(
        &self,
        frame_bgr: &Mat,
        cfg: &TextOcrConfig,
    ) -> Result<Vec<RawTextBlock>, Box<dyn Error>> {
        let w = frame_bgr.cols();
        let h = frame_bgr.rows();

        // 复制一份再涂黑，不修改调用方传进来的原始截图（调用方可能后面
        // 还要拿原图去做坐标/地图识别）。
        let mut masked = frame_bgr.try_clone()?;

        let chat_x = (w as f64 * cfg.chat_box_left_frac) as i32;
        let chat_y = (h as f64 * cfg.chat_box_top_frac) as i32;
        let chat_w = (w as f64 * (cfg.chat_box_right_frac - cfg.chat_box_left_frac)) as i32;
        let chat_h = (h as f64 * (cfg.chat_box_bottom_frac - cfg.chat_box_top_frac)) as i32;
        let chat_rect = core::Rect::new(chat_x, chat_y, chat_w.max(1), chat_h.max(1));

        imgproc::rectangle(
            &mut masked,
            chat_rect,
            core::Scalar::new(0.0, 0.0, 0.0, 0.0),
            -1,
            imgproc::LINE_8,
            0,
        )?;

        let image = mat_bgr_to_dynamic_image(&masked)?;
        let results = self.engine.recognize(&image)?;

        let mut blocks = Vec::with_capacity(results.len());
        for r in results {
            blocks.push(RawTextBlock {
                text: r.text,
                confidence: r.confidence,
                x: r.bbox.rect.left() as i32,
                y: r.bbox.rect.top() as i32,
                w: r.bbox.rect.width() as i32,
                h: r.bbox.rect.height() as i32,
            });
        }
        Ok(blocks)
    }
}

/// 从一份原始 OCR 结果里筛出怪物白名单命中，带位置信息（供攻击瞄准用）。
///
/// 顺带做一次"同名字+位置几乎重叠"去重：OCR 检测阶段偶尔会对同一小块
/// 文字生成两个几乎重叠的候选框，各自识别匹配后就会变成两条内容一样
/// 的记录，保留分数更高的那一条就够了。
pub fn match_monsters(
    blocks: &[RawTextBlock],
    whitelist: &HashSet<String>,
    cfg: &TextOcrConfig,
) -> Vec<(String, TextBox, f32)> {
    let mut matched: Vec<(String, TextBox, f32)> = Vec::new();
    for block in blocks {
        if block.confidence < cfg.min_confidence {
            continue;
        }
        let cleaned = filter_chinese_only(&block.text);
        if cleaned.is_empty() {
            continue;
        }
        let text_box = TextBox {
            x: block.x,
            y: block.y,
            w: block.w,
            h: block.h,
            area: block.w * block.h,
        };
        for name in find_whitelist_matches(&cleaned, whitelist, cfg.max_edit_distance) {
            matched.push((name, text_box, block.confidence));
        }
    }

    let mut deduped: Vec<(String, TextBox, f32)> = Vec::new();
    for (name, rect, score) in matched {
        let dup_idx = deduped
            .iter()
            .position(|(existing_name, existing_rect, _)| {
                existing_name == &name && rects_overlap_significantly(existing_rect, &rect)
            });
        match dup_idx {
            Some(i) if deduped[i].2 < score => deduped[i] = (name, rect, score),
            Some(_) => {}
            None => deduped.push((name, rect, score)),
        }
    }
    deduped
}

/// 两个框是否明显是同一个位置（交叠面积占较小框面积的比例超过阈值）。
fn rects_overlap_significantly(a: &TextBox, b: &TextBox) -> bool {
    let ax2 = a.x + a.w;
    let ay2 = a.y + a.h;
    let bx2 = b.x + b.w;
    let by2 = b.y + b.h;

    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);

    let iw = (ix2 - ix1).max(0);
    let ih = (iy2 - iy1).max(0);
    let inter_area = (iw as i64) * (ih as i64);
    if inter_area <= 0 {
        return false;
    }

    let area_a = (a.w as i64) * (a.h as i64);
    let area_b = (b.w as i64) * (b.h as i64);
    let min_area = area_a.min(area_b).max(1);

    (inter_area as f64) / (min_area as f64) > 0.3
}

/// 从同一份原始 OCR 结果里筛出物品白名单命中。不需要精确坐标
/// （拾取是点固定的"拾取"按钮，不是点物品本身），返回
/// (匹配到的白名单物品名, OCR 原始识别文字, OCR 置信度)。
pub fn match_items(
    blocks: &[RawTextBlock],
    whitelist: &HashSet<String>,
    cfg: &TextOcrConfig,
) -> Vec<(String, String, f32)> {
    let mut matched = Vec::new();
    for block in blocks {
        if block.confidence < cfg.min_confidence {
            continue;
        }
        let cleaned = filter_chinese_only(&block.text);
        if cleaned.is_empty() {
            continue;
        }
        for name in find_whitelist_matches(&cleaned, whitelist, cfg.max_edit_distance) {
            matched.push((name, block.text.clone(), block.confidence));
        }
    }
    matched
}

/// 从同一份 OCR 结果里挑出落在"血量数字"区域内的文字块，解析出
/// "当前/最大"两个数字，算出血量百分比。不需要白名单匹配，直接把
/// 这个区域里识别到的数字解析出来就行。
///
/// 返回 (当前血量, 最大血量, 百分比)；解析失败（文字残缺、格式不对、
/// 区域内没识别到东西）返回 None，调用方自己决定怎么处理（比如沿用
/// 上一次读到的值，不要让血量突然跳变成 0）。
pub fn extract_hp(
    blocks: &[RawTextBlock],
    frame_w: i32,
    frame_h: i32,
    cfg: &TextOcrConfig,
) -> Option<(u32, u32, f32)> {
    let roi_x1 = (frame_w as f64 * cfg.hp_left_frac) as i32;
    let roi_y1 = (frame_h as f64 * cfg.hp_top_frac) as i32;
    let roi_x2 = (frame_w as f64 * cfg.hp_right_frac) as i32;
    let roi_y2 = (frame_h as f64 * cfg.hp_bottom_frac) as i32;

    let mut best: Option<&RawTextBlock> = None;

    for block in blocks {
        if block.confidence < cfg.min_confidence {
            continue;
        }
        let cx = block.x + block.w / 2;
        let cy = block.y + block.h / 2;
        if cx < roi_x1 || cx > roi_x2 || cy < roi_y1 || cy > roi_y2 {
            continue;
        }

        let better = match best {
            Some(b) => block.confidence > b.confidence,
            None => true,
        };
        if better {
            best = Some(block);
        }
    }

    parse_hp_text(&best?.text)
}

/// 把 "2130/2130" 这种格式的文字解析成 (当前, 最大, 百分比)。
/// OCR 偶尔会把 "/" 认错成别的符号，做一点容错：只要文字里能找到
/// 恰好两段连续数字，就当作 (当前, 最大)，不死抠分隔符必须是 "/"。
fn parse_hp_text(text: &str) -> Option<(u32, u32, f32)> {
    let digit_groups: Vec<&str> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();

    if digit_groups.len() != 2 {
        return None;
    }

    let current: u32 = digit_groups[0].parse().ok()?;
    let max: u32 = digit_groups[1].parse().ok()?;

    if max == 0 {
        return None;
    }

    let percent = (current as f32 / max as f32) * 100.0;
    Some((current, max, percent))
}
