// src/game/bot.rs
//
// 核心循环：每一轮截图，喂给 OCR 识别（怪物/物品白名单 + 地图名字）。
// 按钮坐标已经在 startup_check 阶段一次性匹配、缓存好了，这里不用
// 再重新做模板匹配。

use crate::game::app_config::AppConfig;
use crate::game::ocr::text_ocr::{self, TextOcrConfig, TextOcrRecognizer};
use crate::game::state;
use crate::game::util::capture_window;
use std::fs;
use std::thread;
use std::time::Duration;
use xcap::Window;

const CAPTURE_INTERVAL: Duration = Duration::from_millis(500);
const DISABLED_POLL_INTERVAL: Duration = Duration::from_millis(200);
const DEBUG_CAPTURE_DIR: &str = "debug";
const DEBUG_CAPTURE_PATH: &str = "debug/latest_capture.png";

pub fn run(window: Window, ocr_recognizer: TextOcrRecognizer, app_config: AppConfig) {
    let ocr_cfg = TextOcrConfig::default();

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

        if state::is_debug_capture_enabled() {
            if let Err(e) = save_debug_frame(&raw, width, height) {
                println!("⚠️  调试截图存盘失败: {}", e);
            }
        }

        if let Err(e) = run_ocr(&raw, width, height, &ocr_recognizer, &ocr_cfg, &app_config) {
            println!("⚠️  OCR 识别失败，跳过: {}", e);
        }

        thread::sleep(CAPTURE_INTERVAL);
    }
}

fn run_ocr(
    raw: &[u8],
    width: u32,
    height: u32,
    recognizer: &TextOcrRecognizer,
    cfg: &TextOcrConfig,
    app_config: &AppConfig,
) -> Result<(), String> {
    let bgr = text_ocr::rgba_to_bgr_mat(raw, width, height)
        .map_err(|e| format!("转 BGR 图失败: {}", e))?;

    let blocks = recognizer
        .recognize_frame(&bgr, cfg)
        .map_err(|e| format!("OCR 识别失败: {}", e))?;

    if blocks.is_empty() {
        println!("📖 本轮 OCR 未识别到任何文字");
        return Ok(());
    }
    println!("📖 本轮 OCR 识别到 {} 个文字块", blocks.len());

    match text_ocr::match_map_name(&blocks, width as i32, height as i32, cfg) {
        Some((name, score)) => println!("   🗺️  地图名字: {} (置信度 {:.2})", name, score),
        None => println!("   🗺️  地图名字: 未识别到"),
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
