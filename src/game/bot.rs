use crate::game::actions;
use crate::game::button_finder::{ButtonFinder, rgba_to_gray_mat};
use crate::game::state::{self, ButtonInfo};
use crate::game::util::capture_window;
use std::fs;
use std::thread;
use std::time::Duration;
use xcap::Window;

const MATCH_THRESHOLD: f64 = 0.8;
const CAPTURE_INTERVAL: Duration = Duration::from_millis(500);
const DISABLED_POLL_INTERVAL: Duration = Duration::from_millis(200);
const DEBUG_CAPTURE_DIR: &str = "debug";
const DEBUG_CAPTURE_PATH: &str = "debug/latest_capture.png";

pub fn run(window: Window, button_finder: ButtonFinder) {
    loop {
        if !state::is_writer_enabled() {
            thread::sleep(DISABLED_POLL_INTERVAL);
            continue;
        }

        match capture_and_find(&window, &button_finder) {
            Ok((capture_width, buttons)) => {
                state::replace_buttons(buttons.clone());
                print_buttons(&buttons);

                // 先只测试「点击」这个动作本身：识别到 close 就点一下。
                // pickup / attack 什么时候点、按什么节奏点，策略留到下一步再定。
                if let Some(close_btn) = buttons.iter().find(|b| b.name == "close") {
                    if let Err(e) = actions::click_button(&window, capture_width, close_btn) {
                        println!("⚠️  点击 close 失败: {}", e);
                    }
                }
            }
            Err(e) => println!("⚠️  本轮截图/识别失败，跳过: {}", e),
        }

        thread::sleep(CAPTURE_INTERVAL);
    }
}

fn capture_and_find(
    window: &Window,
    button_finder: &ButtonFinder,
) -> Result<(u32, Vec<ButtonInfo>), String> {
    let (raw, width, height) = capture_window(window).ok_or("窗口截图失败（窗口可能已关闭）")?;

    if state::is_debug_capture_enabled() {
        if let Err(e) = save_debug_frame(&raw, width, height) {
            println!("⚠️  调试截图存盘失败: {}", e);
        }
    }

    let gray = rgba_to_gray_mat(&raw, width, height).map_err(|e| format!("转灰度图失败: {}", e))?;

    let buttons = button_finder
        .find_buttons(&gray, width, MATCH_THRESHOLD)
        .map_err(|e| format!("模板匹配失败: {}", e))?;

    Ok((width, buttons))
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
