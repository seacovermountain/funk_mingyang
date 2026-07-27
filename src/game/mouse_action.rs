// src/game/mouse_action.rs
//
// 最基础的鼠标点击动作——只负责"点这一下"，不掺杂任何坐标换算、
// 按钮匹配相关的业务逻辑。Enigo 实例由调用方传入、全局只创建一次，
// 这里不会自己 new 一个新的。

use enigo::{Button, Coordinate::Abs, Direction, Enigo, Mouse};
use std::{thread, time::Duration};

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
