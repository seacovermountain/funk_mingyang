// src/game/actions.rs
//
// 负责把"识别到的按钮"变成真实的鼠标点击。
//
// 关键点：模板匹配算出来的 click_x/click_y 是截图像素坐标系下的坐标
// （Retina 屏幕下是窗口逻辑分辨率的 2 倍），但鼠标点击用的是系统的
// 逻辑坐标系，需要按 窗口逻辑宽度/截图像素宽度 这个比例换算回去，
// 再加上窗口左上角在屏幕上的实际位置。

use crate::game::state::ButtonInfo;
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use xcap::Window;

/// 点击一个已识别到的按钮。
///
/// `capture_width`：这一轮截图的像素宽度，用来把 click_x/click_y 换算回系统逻辑坐标。
pub fn click_button(
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

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("初始化鼠标控制失败: {}", e))?;

    enigo
        .move_mouse(abs_x, abs_y, Coordinate::Abs)
        .map_err(|e| format!("移动鼠标失败: {}", e))?;
    enigo
        .button(Button::Left, Direction::Click)
        .map_err(|e| format!("模拟点击失败: {}", e))?;

    println!(
        "🖱️  点击按钮 [{}] -> 屏幕坐标 ({}, {})",
        button.name, abs_x, abs_y
    );

    Ok(())
}
