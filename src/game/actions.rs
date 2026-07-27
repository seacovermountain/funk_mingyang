// src/game/actions.rs
//
// 负责"该点哪个坐标"这类业务逻辑（坐标换算、窗口位置），真正
// "怎么点下去"委托给 mouse_action::click_at。Enigo 实例由调用方
// 传入，这里不创建。

use crate::game::mouse_action;
use crate::game::state::ButtonInfo;
use enigo::Enigo;
use xcap::Window;

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

    mouse_action::click_at(enigo, center_x, center_y, "切换焦点到游戏窗口");
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

    mouse_action::click_at(enigo, abs_x, abs_y, &format!("点击按钮 [{}]", button.name));
    Ok(())
}
