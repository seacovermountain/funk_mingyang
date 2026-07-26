// src/game/state.rs
//
// 进程级共享状态：
// 1. WRITER_ENABLED —— 写线程总开关，默认开启。
// 2. DEBUG_CAPTURE_ENABLED —— 调试截图存盘开关，默认关闭。
//    开启后，每一轮截图都会覆盖保存到 debug/latest_capture.png，
//    方便随时打开这张图看程序当前到底看到了什么。
// 3. 按钮缓存 —— 最近一次识别到的按钮信息。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static WRITER_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn enable_writer() {
    WRITER_ENABLED.store(true, Ordering::SeqCst);
}

pub fn disable_writer() {
    WRITER_ENABLED.store(false, Ordering::SeqCst);
}

pub fn is_writer_enabled() -> bool {
    WRITER_ENABLED.load(Ordering::SeqCst)
}

/// 调试截图存盘开关。默认 false（关闭）。
/// 目前只能改这里的初始值重新编译来切换，后续要加快捷键实时切换。
static DEBUG_CAPTURE_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn enable_debug_capture() {
    DEBUG_CAPTURE_ENABLED.store(true, Ordering::SeqCst);
}

pub fn disable_debug_capture() {
    DEBUG_CAPTURE_ENABLED.store(false, Ordering::SeqCst);
}

pub fn is_debug_capture_enabled() -> bool {
    DEBUG_CAPTURE_ENABLED.load(Ordering::SeqCst)
}

/// 单个按钮的识别结果。
#[derive(Debug, Clone)]
pub struct ButtonInfo {
    pub name: String,
    /// 匹配到的模板区域左上角坐标（真实窗口坐标）
    pub x: i32,
    pub y: i32,
    /// 匹配到的模板区域尺寸（已按当前分辨率缩放）
    pub width: u32,
    pub height: u32,
    /// 模板匹配置信度，0.0 ~ 1.0
    pub confidence: f64,
    /// 真正应该点击的坐标——大多数按钮等于匹配区域中心，
    /// 但像 minimap 这种，匹配锚点和点击位置不是同一个地方，
    /// 已经按配置里的偏移量算好了。
    pub click_x: i32,
    pub click_y: i32,
    pub updated_at: Instant,
}

static BUTTON_CACHE: OnceLock<Mutex<HashMap<String, ButtonInfo>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, ButtonInfo>> {
    BUTTON_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn replace_buttons(buttons: Vec<ButtonInfo>) {
    let mut map = HashMap::with_capacity(buttons.len());
    for b in buttons {
        map.insert(b.name.clone(), b);
    }
    *cache().lock().unwrap() = map;
}

pub fn get_button(name: &str) -> Option<ButtonInfo> {
    cache().lock().unwrap().get(name).cloned()
}

pub fn snapshot() -> Vec<ButtonInfo> {
    cache().lock().unwrap().values().cloned().collect()
}
