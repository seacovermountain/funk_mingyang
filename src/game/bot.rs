// src/game/bot.rs
//
// 核心循环：每一轮只截一次图，同一份画面分别喂给「按钮匹配」和
// 「OCR 文字识别」两条流水线，互不影响、各自出错互不拖累。

use crate::game::actions;
use crate::game::app_config::AppConfig;
use crate::game::button_finder::{ButtonFinder, rgba_to_gray_mat};
use crate::game::ocr::text_ocr::{self, TextOcrConfig, TextOcrRecognizer};
use crate::game::state::{self, ButtonInfo};
use crate::game::util::capture_window;
use enigo::Enigo;
use std::fs;
use std::thread;
use std::time::Duration;
use xcap::Window;

const MATCH_THRESHOLD: f64 = 0.8;
const CAPTURE_INTERVAL: Duration = Duration::from_millis(500);
const DISABLED_POLL_INTERVAL: Duration = Duration::from_millis(200);
const DEBUG_CAPTURE_DIR: &str = "debug";
const DEBUG_CAPTURE_PATH: &str = "debug/latest_capture.png";

pub fn run(
    window: Window,
    button_finder: ButtonFinder,
    ocr_recognizer: TextOcrRecognizer,
    app_config: AppConfig,
    mut enigo: Enigo,
) {
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

        // ---- 按钮识别 ----
        match run_button_matching(&raw, width, height, &button_finder) {
            Ok(buttons) => {
                state::replace_buttons(buttons.clone());
                print_buttons(&buttons);

                // 先只测试「点击」这个动作本身：识别到 close 就点一下。
                // pickup / attack 什么时候点、按什么节奏点，策略留到下一步再定。
                if let Some(close_btn) = buttons.iter().find(|b| b.name == "close") {
                    if let Err(e) = actions::click_button(&mut enigo, &window, width, close_btn) {
                        println!("⚠️  点击 close 失败: {}", e);
                    }
                }
            }
            Err(e) => println!("⚠️  按钮识别失败，跳过: {}", e),
        }

        // ---- OCR 文字识别（怪物/物品白名单 + 地图名字，先只打印）----
        if let Err(e) = run_ocr(&raw, width, height, &ocr_recognizer, &ocr_cfg, &app_config) {
            println!("⚠️  OCR 识别失败，跳过: {}", e);
        }

        thread::sleep(CAPTURE_INTERVAL);
    }
}

fn run_button_matching(
    raw: &[u8],
    width: u32,
    height: u32,
    button_finder: &ButtonFinder,
) -> Result<Vec<ButtonInfo>, String> {
    let gray = rgba_to_gray_mat(raw, width, height).map_err(|e| format!("转灰度图失败: {}", e))?;
    button_finder
        .find_buttons(&gray, width, MATCH_THRESHOLD)
        .map_err(|e| format!("模板匹配失败: {}", e))
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

fn print_buttons(buttons: &[ButtonInfo]) {
    if buttons.is_empty() {
        println!("🔍 本轮未识别到任何按钮");
        return;
    }
    println!("🔍 本轮识别到 {} 个按钮：", buttons.len());
    for b in buttons {
        println!(
            "   - {:<10} 匹配区域 ({:>4}, {:>4}) {:>3}x{:<3}  点击位置 ({:>4}, {:>4})  置信度 {:.2}",
            b.name, b.x, b.y, b.width, b.height, b.click_x, b.click_y, b.confidence
        );
    }
}
