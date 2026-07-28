// src/game/bot.rs
//
// 核心循环：每一轮截图，喂给 OCR 识别（怪物/物品白名单 + 地图名字）。
// 按钮坐标已经在 startup_check 阶段一次性匹配、缓存好了，这里不用
// 再重新做模板匹配。

use crate::game::app_config::AppConfig;
use crate::game::ocr::text_ocr::{self, TextOcrConfig, TextOcrRecognizer};
use crate::game::position_reader::{self, DigitTemplate, PositionReaderConfig};
use crate::game::state;
use crate::game::util::capture_window;
use chrono::Local;
use opencv::prelude::*;
use std::fs;
use std::thread;
use std::time::Duration;
use xcap::Window;
const CAPTURE_INTERVAL: Duration = Duration::from_millis(500);
const DISABLED_POLL_INTERVAL: Duration = Duration::from_millis(200);
const DEBUG_CAPTURE_DIR: &str = "debug";
const DEBUG_CAPTURE_PATH: &str = "debug/latest_capture.png";
/// 坐标数字模板匹配的最低置信度——实测真实数字普遍在 98% 以上，
/// 卡在 0.85 既能滤掉噪点，也给字体渲染差异留了余量。
const POSITION_MIN_CONFIDENCE: f32 = 0.70;

pub fn run(
    window: Window,
    ocr_recognizer: TextOcrRecognizer,
    app_config: AppConfig,
    digit_templates: Vec<DigitTemplate>,
) {
    let ocr_cfg = TextOcrConfig::default();
    let position_cfg = PositionReaderConfig::default();

    loop {
        if !state::is_writer_enabled() {
            thread::sleep(DISABLED_POLL_INTERVAL);
            continue;
        }

        let (raw, width, height) = match capture_window(&window) {
            Some(frame) => frame,
            None => {
                println!("⚠️  窗口截图失败（窗口可能已关闭），跳过本轮");
                thread::sleep(CAPTURE_INTERVAL);
                continue;
            }
        };
        // 时间戳在截图刚拿到手时立刻记下——代表"这一帧画面是什么时候截的"，
        // 不是"处理到第几步的时候"，避免后面存盘/识别耗时把时间戳拖晚。
        let captured_at = Local::now();

        if state::is_debug_capture_enabled() {
            if let Err(e) = save_debug_frame(&raw, width, height) {
                println!("⚠️  调试截图存盘失败: {}", e);
            }
        }

        println!("\n⏰ {}", captured_at.format("%Y-%m-%d %H:%M:%S%.3f"));

        // 这一帧只转一次 BGR，OCR 文字识别和坐标数字识别共用，不重复转换。
        match text_ocr::rgba_to_bgr_mat(&raw, width, height) {
            Ok(bgr) => {
                if let Err(e) = run_ocr(&bgr, &ocr_recognizer, &ocr_cfg, &app_config) {
                    println!("⚠️  OCR 识别失败，跳过: {}", e);
                }
                run_position(&bgr, &digit_templates, &position_cfg);
            }
            Err(e) => println!("⚠️  转 BGR 图失败，本轮跳过识别: {}", e),
        }

        thread::sleep(CAPTURE_INTERVAL);
    }
}

/// 识别人物当前坐标并打印。识别不到就打印"未识别到"，不中断主循环。
fn run_position(bgr: &opencv::core::Mat, templates: &[DigitTemplate], cfg: &PositionReaderConfig) {
    match position_reader::read_position(bgr, cfg, templates, POSITION_MIN_CONFIDENCE) {
        Some((x, y)) => println!("   📍 人物坐标: ({}, {})", x, y),
        None => println!("   📍 人物坐标: 未识别到"),
    }
}

fn run_ocr(
    bgr: &opencv::core::Mat,
    recognizer: &TextOcrRecognizer,
    cfg: &TextOcrConfig,
    app_config: &AppConfig,
) -> Result<(), String> {
    let width = bgr.cols();
    let height = bgr.rows();

    let blocks = recognizer
        .recognize_frame(bgr, cfg)
        .map_err(|e| format!("OCR 识别失败: {}", e))?;

    if blocks.is_empty() {
        println!("📖 本轮 OCR 未识别到任何文字");
        return Ok(());
    }
    println!("📖 本轮 OCR 识别到 {} 个文字块", blocks.len());

    match text_ocr::match_map_name(&blocks, width, height, cfg) {
        Some((name, score)) => println!("   🗺️  地图名字: {} (置信度 {:.2})", name, score),
        None => println!("   🗺️  地图名字: 未识别到"),
    }

    match text_ocr::extract_hp(&blocks, width, height, cfg) {
        Some((cur, max, pct)) => println!("   ❤️  血量: {}/{} ({:.1}%)", cur, max, pct),
        None => println!("   ❤️  血量: 未识别到"),
    }

    let monsters = text_ocr::match_monsters(&blocks, &app_config.target_monsters, cfg);
    if monsters.is_empty() {
        println!("   🐲 怪物白名单命中: 无");
    } else {
        for (name, pos, score) in &monsters {
            println!(
                "   🐲 怪物命中: {:<8} 位置 ({}, {}) {}x{}  置信度 {:.2}",
                name, pos.x, pos.y, pos.w, pos.h, score
            );
        }
    }

    let items = text_ocr::match_items(&blocks, &app_config.target_items, cfg);
    if items.is_empty() {
        println!("   💰 物品白名单命中: 无");
    } else {
        for (name, raw_text, score) in &items {
            println!(
                "   💰 物品命中: {:<8} (原始识别: \"{}\"，置信度 {:.2})",
                name, raw_text, score
            );
        }
    }

    Ok(())
}

fn save_debug_frame(raw: &[u8], width: u32, height: u32) -> Result<(), String> {
    fs::create_dir_all(DEBUG_CAPTURE_DIR).map_err(|e| format!("创建调试目录失败: {}", e))?;

    let img = image::RgbaImage::from_raw(width, height, raw.to_vec())
        .ok_or("原始像素数据和宽高对不上，无法转成图片")?;

    img.save(DEBUG_CAPTURE_PATH)
        .map_err(|e| format!("保存调试截图失败: {}", e))
}
