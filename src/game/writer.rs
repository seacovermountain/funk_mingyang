// src/game/writer.rs
//
// 写线程：截图 -> OCR/坐标识别 -> 把这一轮识别结果写进共享状态
// (state::set_latest_game_info)，覆盖式更新。
//
// 🔄 不再用 channel 发给读线程——channel 会让读线程的节奏被写线程锁死
// (写一次、读线程才能动一次)，写线程识别一帧耗时比较长，读线程就会
// 跟着"一步一停"，拾取/攻击/寻路这些点击动作全都会被拖得断断续续。
// 现在写线程只管尽量快地持续更新共享状态，读线程(reader.rs)按自己
// 更快、更固定的节奏去读这个共享状态做决策，两边互不阻塞。
//
// 受 state::is_writer_enabled() 这个全局开关控制：
// 关闭时只是睡一下继续循环检查，线程本身不退出，随时可以被重新打开。

use crate::game::app_config::AppConfig;
use crate::game::ocr::{self, TextOcrConfig, TextOcrRecognizer};
use crate::game::position_reader::{self, DigitTemplate, PositionReaderConfig};
use crate::game::state::{self, GameInfo};
use crate::game::util::{capture_window, rgba_to_bgr_mat};
use chrono::Local;
use opencv::prelude::*;
use std::thread;
use std::time::{Duration, Instant};
use xcap::Window;

const CAPTURE_INTERVAL: Duration = Duration::from_millis(500);
const DISABLED_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// 坐标数字模板匹配的最低置信度。数字字符模板大、匹配稳定在 98% 以上，
/// 但逗号模板只有 3x6 像素，天然比数字更容易得分偏低（实测常见 75%~85%），
/// 卡太高会把逗号本身过滤掉，导致坐标解析（split(',')）整体失败。
const POSITION_MIN_CONFIDENCE: f32 = 0.70;

pub fn run(
    window: Window,
    ocr_recognizer: TextOcrRecognizer,
    app_config: AppConfig,
    digit_templates: Vec<DigitTemplate>,
) {
    let ocr_cfg = TextOcrConfig::default();
    let position_cfg = PositionReaderConfig::default();
    // 沿用上一轮成功识别到的地图名字/血量/坐标，避免偶尔一帧没识别到
    // 就把这些值清空、误传给读线程（比如血量突然"跳变"成 0）。
    let mut previous = GameInfo::default();

    loop {
        if !state::is_writer_enabled() {
            thread::sleep(DISABLED_POLL_INTERVAL);
            continue;
        }

        let (raw, width, height) = match capture_window(&window) {
            Some(frame) => frame,
            None => {
                println!("⚠️  [写线程] 窗口截图失败（窗口可能已关闭），跳过本轮");
                thread::sleep(CAPTURE_INTERVAL);
                continue;
            }
        };
        let captured_at = Local::now();

        let bgr = match rgba_to_bgr_mat(&raw, width, height) {
            Ok(m) => m,
            Err(e) => {
                println!("⚠️  [写线程] 转 BGR 图失败，跳过本轮: {}", e);
                thread::sleep(CAPTURE_INTERVAL);
                continue;
            }
        };

        println!(
            "⏰ [写线程] {}",
            captured_at.format("%Y-%m-%d %H:%M:%S%.3f")
        );

        let info = recognize_game_info(
            &bgr,
            &ocr_recognizer,
            &ocr_cfg,
            &app_config,
            &digit_templates,
            &position_cfg,
            &previous,
        );

        previous = info.clone();

        // 覆盖式写入共享状态——读线程随时按自己的节奏来读最新这一份，
        // 不需要这里等谁来"收"，写完立刻进入下一轮截图识别。
        state::set_latest_game_info(info);

        thread::sleep(CAPTURE_INTERVAL);
    }
}

/// 对这一帧画面做完整识别（地图名字、血量、怪物/物品白名单命中、人物坐标），
/// 拼成一份 GameInfo 快照返回。identify 不到的字段用 previous 里的旧值兜底。
fn recognize_game_info(
    bgr: &opencv::core::Mat,
    recognizer: &TextOcrRecognizer,
    ocr_cfg: &TextOcrConfig,
    app_config: &AppConfig,
    digit_templates: &[DigitTemplate],
    position_cfg: &PositionReaderConfig,
    previous: &GameInfo,
) -> GameInfo {
    let width = bgr.cols();
    let height = bgr.rows();

    let blocks = match recognizer.recognize_frame(bgr, ocr_cfg) {
        Ok(b) => b,
        Err(e) => {
            println!("⚠️  [写线程] OCR 识别失败: {}", e);
            Vec::new()
        }
    };

    let map_name = ocr::match_map_name(&blocks, width, height, ocr_cfg)
        .map(|(name, _)| name)
        .unwrap_or_else(|| previous.map_name.clone());

    let hp_percent = ocr::extract_hp(&blocks, width, height, ocr_cfg)
        .map(|(_, _, pct)| pct.round().clamp(0.0, 100.0) as u8)
        .unwrap_or(previous.hp_percent);

    let monster_matches = ocr::match_monsters(&blocks, &app_config.target_monsters, ocr_cfg);
    // 🛡️ 顺手把每个匹配到的怪物名字框位置也存下来，移动模块寻路点击
    // 前会拿这些框做规避判断——点在活着的怪物身上，游戏经常会判定成
    // "选中目标"而不是"移动"，人物会纹丝不动。
    let monster_boxes = monster_matches
        .iter()
        .map(|(_, text_box, _)| (text_box.x, text_box.y, text_box.w, text_box.h))
        .collect();
    let monsters = monster_matches
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();

    let items = ocr::match_items(&blocks, &app_config.target_items, ocr_cfg)
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();

    // 🎯 先记住这一帧是不是真的读到了坐标，再决定要不要用旧值兜底——
    // 下游(卡住检测)必须能区分"这是这一帧刚读到的新坐标"还是"读失败
    // 沿用的旧坐标"，否则会把"没读到"误判成"角色没动"。
    let fresh_position =
        position_reader::read_position(bgr, position_cfg, digit_templates, POSITION_MIN_CONFIDENCE);
    let position_fresh = fresh_position.is_some();
    let player_position = fresh_position.or(previous.player_position);

    GameInfo {
        map_name,
        player_position,
        hp_percent,
        monsters,
        items,
        updated_at: Some(Instant::now()),
        capture_width: width as u32,
        capture_height: height as u32,
        position_fresh,
        monster_boxes,
    }
}
