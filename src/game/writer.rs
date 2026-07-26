// src/game/writer.rs
//
// 写线程主循环：截图 -> 转灰度 -> 模板匹配找按钮 -> 写入全局缓存 -> 通知读线程。
//
// 受 state::is_writer_enabled() 这个全局开关控制：
// 关闭时跳过"截图/识别"这一整套，只是睡一下继续循环，线程本身不退出，
// 方便随时被重新打开。

use crate::game::button_finder::{ButtonFinder, rgba_to_gray_mat};
use crate::game::state::{self, ButtonSnapshot};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};
use xcap::Window;

/// 开关关闭时，多久检查一次是否被重新打开。
const DISABLED_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// 开关开启时，两次截图识别之间的间隔。
const CAPTURE_INTERVAL: Duration = Duration::from_millis(200);
/// 模板匹配置信度阈值，低于这个分数不算"找到"这个按钮。
const MATCH_THRESHOLD: f64 = 0.8;

/// 写线程主循环。
///
/// `window`：已经定位好的游戏窗口。
/// `button_finder`：加载好模板的按钮识别器。
/// `tx`：识别结果发给读线程的 channel。
pub fn run(window: Window, button_finder: ButtonFinder, tx: Sender<ButtonSnapshot>) {
    loop {
        if !state::is_writer_enabled() {
            thread::sleep(DISABLED_POLL_INTERVAL);
            continue;
        }
        thread::sleep(CAPTURE_INTERVAL);
    }
}
