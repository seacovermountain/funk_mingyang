// src/game/reader.rs
//
// 读线程：按固定节奏(ACTION_INTERVAL)不停去读写线程更新的"最新识别
// 结果"(state::get_latest_game_info)，驱动"拾取 > 打怪 > 寻路"的
// 自动化决策。
//
// 🔄 不再通过 channel 接收数据——之前用 channel 时，读线程的决策节奏
// 被写线程的识别速度完全锁死(写一次、读线程才能醒来动一次)。现在读
// 线程按自己的固定节奏去读共享状态，哪怕写线程这一轮识别还没出新
// 结果，拾取/攻击也会拿着"当前已知的最新信息"继续点击；但移动点击
// 只在真的拿到一帧新坐标时才执行，避免对着同一个旧坐标反复点击，
// 把游戏自己的寻路指令打断重算。
//
// 🧭 Enigo（鼠标控制）在这个线程里创建/持有——底层持有系统句柄，
// 稳妥起见全程只在一个线程里用，不跨线程传递。

use crate::game::app_config::AppConfig;
use crate::game::movement::MovementConfig;
use crate::game::state::{self, GameInfo};
use crate::game::{actions, movement};
use enigo::Enigo;
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};
use xcap::Window;

/// 🔄 读线程自己的决策节奏——跟写线程识别一帧要多久完全无关，这个值
/// 只决定"拾取/攻击/寻路这些点击动作多久补一次"。
const ACTION_INTERVAL: Duration = Duration::from_millis(300);

/// 巡逻卡住检测：跟上一次真实读到的坐标相比，变化小于这个值就算
/// "没动"(容忍坐标识别本身的量化误差，不要求严格相等)。
const STUCK_POSITION_EPS: f64 = 2.0;

/// 连续卡住超过这么多次，就放弃当前这个巡逻点、跳到下一个，避免
/// 死循环卡死在同一个点上出不去。这里的"次数"是按"识别到新一帧"计的。
const MAX_STALL_BEFORE_SKIP: u32 = 20;

/// 🎥 录制巡逻路径时，跟上一个记录点的距离要超过这个值才记一个新点
/// (世界坐标单位)。
const MIN_RECORD_DISTANCE: f64 = 10.0;

/// 单张地图的巡逻卡住检测状态。只在读线程这一个线程里使用。
#[derive(Default)]
struct PatrolStuckState {
    last_pos: Option<(i32, i32)>,
    /// 连续"坐标没怎么变"的次数，直接当作 retry_offset 传给
    /// `movement::move_toward`，驱动它换方向/换半径重试。
    stall_streak: u32,
    /// 上一次已经纳入卡住判断的那一帧的时间戳——轮询比写线程识别快
    /// 得多，同一帧 GameInfo 会被读线程看到很多次，必须靠这个时间戳
    /// 判断"这是不是一帧新的识别结果"。
    last_evaluated_frame: Option<Instant>,
}

pub fn run(mut enigo: Enigo, window: Window, app_config: AppConfig) {
    let movement_cfg = MovementConfig::default();
    let mut stuck_states: HashMap<String, PatrolStuckState> = HashMap::new();
    let mut was_recording = false;
    let mut last_printed_frame: Option<Instant> = None;

    loop {
        let info = state::get_latest_game_info();

        if info.updated_at != last_printed_frame {
            print_snapshot(&info);
            last_printed_frame = info.updated_at;
        }

        let now_recording = state::is_recording_enabled();
        if now_recording {
            if let Some(pos) = info.player_position {
                let should_record = match state::last_recorded_point() {
                    None => true,
                    Some(last) => {
                        let dx = (pos.0 - last.0) as f64;
                        let dy = (pos.1 - last.1) as f64;
                        (dx * dx + dy * dy).sqrt() >= MIN_RECORD_DISTANCE
                    }
                };
                if should_record {
                    state::push_recorded_point(pos);
                    println!(
                        "   📍 [路径录制] 记录路点 #{}: {:?}",
                        state::recorded_point_count(),
                        pos
                    );
                }
            }
        }
        if was_recording && !now_recording {
            let points = state::take_recorded_points();
            dump_recorded_route(&info.map_name, &points);
        }
        was_recording = now_recording;

        if now_recording {
            thread::sleep(ACTION_INTERVAL);
            continue;
        }

        if !info.items.is_empty() {
            click_named_button(&mut enigo, &window, &info, "pickup");
        } else if !info.monsters.is_empty() {
            click_named_button(&mut enigo, &window, &info, "attack");
        } else {
            patrol(
                &info,
                &mut enigo,
                &window,
                &app_config,
                &movement_cfg,
                &mut stuck_states,
            );
        }

        thread::sleep(ACTION_INTERVAL);
    }
}

fn print_snapshot(info: &GameInfo) {
    let map_name = if info.map_name.is_empty() {
        "未知"
    } else {
        &info.map_name
    };

    let position = match info.player_position {
        Some((x, y)) => format!("({}, {})", x, y),
        None => "未知".to_string(),
    };

    let monsters = if info.monsters.is_empty() {
        "无".to_string()
    } else {
        info.monsters.join(", ")
    };

    let items = if info.items.is_empty() {
        "无".to_string()
    } else {
        info.items.join(", ")
    };

    println!(
        "📋 [读线程] 地图: {} | 坐标: {} | 血量: {}% | 怪物: {} | 物品: {}",
        map_name, position, info.hp_percent, monsters, items
    );
}

fn dump_recorded_route(map_name: &str, points: &[(i32, i32)]) {
    if points.is_empty() {
        println!("🎥 [路径录制] 没录到任何点(可能全程都没有有效坐标)，不生成文件");
        return;
    }

    let map_label = if map_name.is_empty() {
        "未知地图"
    } else {
        map_name
    };

    let mut toml_text = String::new();
    toml_text.push_str("[[patrol]]\n");
    toml_text.push_str(&format!("map = \"{}\"\n", map_label));
    toml_text.push_str("points = [\n");
    for (x, y) in points {
        toml_text.push_str(&format!("    [{}, {}],\n", x, y));
    }
    toml_text.push_str("]\n");

    println!(
        "🎥 [路径录制] 录到 {} 个点，生成的配置如下（可以直接复制进 config.toml 的 patrol 部分）：",
        points.len()
    );
    println!("{}", toml_text);

    let filename = format!("patrol_recorded_{}.toml", map_label);
    match std::fs::write(&filename, &toml_text) {
        Ok(_) => println!(
            "🎥 [路径录制] 同时已存到文件: {}（跟可执行文件同目录，不用从终端里抄）",
            filename
        ),
        Err(e) => println!(
            "⚠️  [路径录制] 存文件失败: {}，请直接复制上面终端里打印的内容",
            e
        ),
    }
}

fn click_named_button(enigo: &mut Enigo, window: &Window, info: &GameInfo, name: &str) {
    let Some(button) = state::get_button(name) else {
        println!(
            "   ⚠️  [读线程] 按钮 [{}] 还没识别到缓存坐标，跳过这一次",
            name
        );
        return;
    };
    if info.capture_width == 0 {
        println!("   ⚠️  [读线程] 截图宽度异常，跳过点击 [{}]", name);
        return;
    }
    if let Err(e) = actions::click_button(enigo, window, info.capture_width, &button) {
        println!("   ⚠️  [读线程] 点击按钮 [{}] 失败: {}", name, e);
    }
}

fn patrol(
    info: &GameInfo,
    enigo: &mut Enigo,
    window: &Window,
    app_config: &AppConfig,
    movement_cfg: &MovementConfig,
    stuck_states: &mut HashMap<String, PatrolStuckState>,
) {
    if info.map_name.is_empty() {
        return;
    }

    let Some(route) = app_config.patrol_route_for(&info.map_name) else {
        return;
    };

    if route.points.is_empty() {
        return;
    }

    let Some(cur_pos) = info.player_position else {
        return;
    };

    let idx = state::get_patrol_index(&info.map_name);
    let target = route.points[idx.min(route.points.len() - 1)];

    let dx = (target.0 - cur_pos.0) as f64;
    let dy = (target.1 - cur_pos.1) as f64;
    let dist = (dx * dx + dy * dy).sqrt();

    if dist <= movement::ARRIVE_TOLERANCE {
        println!(
            "   🚩 [读线程] 已到达巡逻点 {:?}({}/{}), 切换下一个点",
            target,
            idx + 1,
            route.points.len()
        );
        state::advance_patrol_index(&info.map_name, route.points.len());
        stuck_states.remove(&info.map_name);
        return;
    }

    let is_new_frame = info.updated_at.is_some()
        && stuck_states
            .get(&info.map_name)
            .and_then(|s| s.last_evaluated_frame)
            != info.updated_at;

    if !is_new_frame {
        return;
    }

    let retry_offset = if info.position_fresh {
        let stuck = stuck_states.entry(info.map_name.clone()).or_default();
        stuck.last_evaluated_frame = info.updated_at;

        let moved_since_last = match stuck.last_pos {
            Some(last) => {
                let mdx = (cur_pos.0 - last.0) as f64;
                let mdy = (cur_pos.1 - last.1) as f64;
                (mdx * mdx + mdy * mdy).sqrt()
            }
            None => f64::MAX,
        };
        stuck.last_pos = Some(cur_pos);

        if moved_since_last < STUCK_POSITION_EPS {
            stuck.stall_streak += 1;
        } else {
            stuck.stall_streak = 0;
        }

        if stuck.stall_streak >= MAX_STALL_BEFORE_SKIP {
            println!(
                "   🧱 [读线程] 巡逻点 {:?} 连续卡住 {} 帧，放弃这个点，跳到下一个",
                target, stuck.stall_streak
            );
            state::advance_patrol_index(&info.map_name, route.points.len());
            stuck_states.remove(&info.map_name);
            return;
        }

        stuck.stall_streak
    } else {
        let stuck = stuck_states.entry(info.map_name.clone()).or_default();
        stuck.last_evaluated_frame = info.updated_at;
        stuck.stall_streak
    };

    if let Err(e) = movement::move_toward(enigo, window, info, target, movement_cfg, retry_offset) {
        println!("   ⚠️  [读线程] 寻路移动失败: {}", e);
    }
}
