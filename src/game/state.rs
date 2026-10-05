// src/game/state.rs
//
// 进程级共享状态：
// 1. LATEST_GAME_INFO —— 写线程/读线程解耦用的共享最新识别结果。
// 2. WRITER_ENABLED —— 写线程总开关，默认开启。
// 3. DEBUG_CAPTURE_ENABLED —— 调试截图存盘开关，默认关闭。
// 4. PATROL_PROGRESS —— 每张地图巡逻到第几个点了。
// 5. RECORDING_ENABLED / RECORDED_POINTS —— 巡逻路径录制开关和坐标存储。
// 6. 按钮缓存 —— 最近一次识别到的按钮信息。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// 游戏当前状态快照，写线程识别后填充，读线程通过共享状态读取使用。
#[derive(Debug, Clone)]
pub struct GameInfo {
    pub map_name: String,
    pub player_position: Option<(i32, i32)>,
    pub hp_percent: u8,
    pub monsters: Vec<String>,
    pub items: Vec<String>,
    pub updated_at: Option<Instant>,
    /// 这一帧截图的物理像素宽高。移动模块要把"世界坐标偏移量"换算成
    /// "该点屏幕上哪个像素"，换算时需要知道这一帧截图相对窗口逻辑
    /// 分辨率的缩放比例——跟 actions::click_button 用 capture_width
    /// 换算按钮点击坐标是同一个道理。
    pub capture_width: u32,
    pub capture_height: u32,
    /// 🎯 player_position 是不是这一帧真的读到的。读坐标失败时 writer
    /// 会沿用上一帧的旧坐标兜底(避免坐标突然变成"未知")，但下游
    /// (卡住检测)必须能分辨"这是新读到的"还是"这是旧值"，否则会把
    /// "读失败"误判成"角色没动"。
    pub position_fresh: bool,
    /// 🛡️ 当前画面里识别到的怪物名字框位置(x,y,w,h，截图像素坐标系)。
    /// 移动模块寻路点击前会检查候选点是不是落在这些框(往四周/往下
    /// 扩一圈,近似怪物本体站立的范围)里——点在活着的怪物身上，游戏
    /// 经常会判定成"选中目标"而不是"移动"，人物会纹丝不动。
    pub monster_boxes: Vec<(i32, i32, i32, i32)>,
}

impl Default for GameInfo {
    fn default() -> Self {
        GameInfo {
            map_name: String::new(),
            player_position: None,
            hp_percent: 100,
            monsters: Vec::new(),
            items: Vec::new(),
            updated_at: None,
            capture_width: 0,
            capture_height: 0,
            position_fresh: false,
            monster_boxes: Vec::new(),
        }
    }
}

/// 🔄 写线程和读线程之间不再用 channel 传递数据——channel 会导致读
/// 线程的节奏被写线程锁死(写一次、读线程才能动一次)，写线程识别一帧
/// 耗时比较长，读线程就会跟着"一步一停"，拾取/攻击/寻路这些点击动作
/// 全都会被拖得断断续续。
///
/// 现在改成:写线程把每一轮识别结果存到这里(覆盖式更新)，读线程
/// 按自己的节奏(更快、更固定的间隔)不停来读最新值去决策点击——写
/// 多快是写线程的事，读多快是读线程自己的事，两边互不阻塞。
static LATEST_GAME_INFO: OnceLock<Mutex<GameInfo>> = OnceLock::new();

fn latest_game_info_cell() -> &'static Mutex<GameInfo> {
    LATEST_GAME_INFO.get_or_init(|| Mutex::new(GameInfo::default()))
}

pub fn set_latest_game_info(info: GameInfo) {
    *latest_game_info_cell().lock().unwrap() = info;
}

pub fn get_latest_game_info() -> GameInfo {
    latest_game_info_cell().lock().unwrap().clone()
}

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

/// 🚩 巡逻进度:记录"每张地图巡逻到第几个点了"，跨帧保持，一张地图
/// 走完最后一个点后循环回第一个点。key 是地图名字。
static PATROL_PROGRESS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

fn patrol_cell() -> &'static Mutex<HashMap<String, usize>> {
    PATROL_PROGRESS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get_patrol_index(map_name: &str) -> usize {
    *patrol_cell().lock().unwrap().get(map_name).unwrap_or(&0)
}

pub fn advance_patrol_index(map_name: &str, total_points: usize) {
    if total_points == 0 {
        return;
    }
    let mut map = patrol_cell().lock().unwrap();
    let cur = map.entry(map_name.to_string()).or_insert(0);
    *cur = (*cur + 1) % total_points;
}

/// 🎥 巡逻路径录制总开关。按热键(F9)切换，默认关闭。
static RECORDING_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn is_recording_enabled() -> bool {
    RECORDING_ENABLED.load(Ordering::SeqCst)
}

pub fn toggle_recording() -> bool {
    let new_val = !RECORDING_ENABLED.load(Ordering::SeqCst);
    RECORDING_ENABLED.store(new_val, Ordering::SeqCst);
    new_val
}

static RECORDED_POINTS: OnceLock<Mutex<Vec<(i32, i32)>>> = OnceLock::new();

fn recorded_points_cell() -> &'static Mutex<Vec<(i32, i32)>> {
    RECORDED_POINTS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn push_recorded_point(p: (i32, i32)) {
    recorded_points_cell().lock().unwrap().push(p);
}

pub fn last_recorded_point() -> Option<(i32, i32)> {
    recorded_points_cell().lock().unwrap().last().copied()
}

pub fn recorded_point_count() -> usize {
    recorded_points_cell().lock().unwrap().len()
}

pub fn take_recorded_points() -> Vec<(i32, i32)> {
    std::mem::take(&mut *recorded_points_cell().lock().unwrap())
}

/// 单个按钮的识别结果。
#[derive(Debug, Clone)]
pub struct ButtonInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub confidence: f64,
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

pub fn insert_buttons(buttons: Vec<ButtonInfo>) {
    let mut map = cache().lock().unwrap();
    for b in buttons {
        map.insert(b.name.clone(), b);
    }
}
