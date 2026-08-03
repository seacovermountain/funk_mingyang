// src/game/actions.rs
//
// 鼠标动作模块：既包含"该点哪个坐标"这类业务逻辑（坐标换算、窗口
// 位置），也包含最基础的"怎么点下去"（click_at）。Enigo 实例由
// 调用方传入、全局只创建一次，这里不会自己 new 一个新的。

use crate::game::state::ButtonInfo;
use enigo::{Button, Coordinate::Abs, Direction, Enigo, Mouse};
use std::{thread, time::Duration};
use xcap::Window;

/// 移动到 (x, y)，停顿一下，按下，再停顿一下，松开。
/// 这个"停顿"是必须的——瞬间按下松开的点击，在这个游戏客户端上
/// 会被直接丢弃，不会有任何反应（已经实测验证过）。
pub fn click_at(enigo: &mut Enigo, x: i32, y: i32, action: &str) {
    let _ = enigo.move_mouse(x, y, Abs);
    thread::sleep(Duration::from_millis(80));
    let _ = enigo.button(Button::Left, Direction::Press);
    thread::sleep(Duration::from_millis(100));
    let _ = enigo.button(Button::Left, Direction::Release);
    println!("🖱️  鼠标动作: {} -> ({}, {})", action, x, y);
}

/// 点一下窗口正中央，让操作系统把这个窗口切换成当前焦点窗口。
/// 只在程序启动、刚找到窗口的时候调用一次。
pub fn activate_window(enigo: &mut Enigo, window: &Window) -> Result<(), String> {
    let window_x = window
        .x()
        .map_err(|e| format!("拿不到窗口 x 坐标: {}", e))?;
    let window_y = window
        .y()
        .map_err(|e| format!("拿不到窗口 y 坐标: {}", e))?;
    let window_width = window
        .width()
        .map_err(|e| format!("拿不到窗口宽度: {}", e))?;
    let window_height = window
        .height()
        .map_err(|e| format!("拿不到窗口高度: {}", e))?;

    let center_x = window_x + (window_width as i32) / 2;
    let center_y = window_y + (window_height as i32) / 2;

    click_at(enigo, center_x, center_y, "切换焦点到游戏窗口");
    Ok(())
}

/// 点击一个已识别到的按钮。
/// `capture_width`：这一轮截图的像素宽度，用来把 click_x/click_y 换算回系统逻辑坐标。
pub fn click_button(
    enigo: &mut Enigo,
    window: &Window,
    capture_width: u32,
    button: &ButtonInfo,
) -> Result<(), String> {
    let logical_width = window
        .width()
        .map_err(|e| format!("拿不到窗口逻辑宽度: {}", e))?;
    let window_x = window
        .x()
        .map_err(|e| format!("拿不到窗口 x 坐标: {}", e))?;
    let window_y = window
        .y()
        .map_err(|e| format!("拿不到窗口 y 坐标: {}", e))?;

    let scale = logical_width as f64 / capture_width as f64;
    let abs_x = window_x + (button.click_x as f64 * scale).round() as i32;
    let abs_y = window_y + (button.click_y as f64 * scale).round() as i32;

    click_at(enigo, abs_x, abs_y, &format!("点击按钮 [{}]", button.name));
    Ok(())
}
