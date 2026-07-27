// src/game/startup_check.rs
//
// 启动自检：进正式业务逻辑之前，确认"识别 + 点击"这套链路真的是通的。
//
// 流程：
// 1. 匹配 attack / pickup / minimap 三个常驻按钮，三个都出现才算过
// 2. 点一下 minimap，把地图打开
// 3. 匹配 close 按钮，出现说明点击这个动作本身也生效了
// 4. 点一下 close，把地图关掉，恢复正常游戏画面
//
// 任何一步等太久还没等到，直接报警并停住，交给人工处理——不自动重试，
// 也不会硬着头皮继续往下跑。

use crate::game::actions;
use crate::game::alarm;
use crate::game::button_finder::{ButtonFinder, rgba_to_gray_mat};
use crate::game::state::{self, ButtonInfo};
use crate::game::util::capture_window;
use enigo::Enigo;
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};
use xcap::Window;

const MATCH_THRESHOLD: f64 = 0.8;
const POLL_INTERVAL: Duration = Duration::from_millis(300);
const TIMEOUT: Duration = Duration::from_secs(10);

pub fn run(window: &Window, button_finder: &ButtonFinder, enigo: &mut Enigo) {
    println!("🔧 [启动自检] 验证常驻按钮 attack / pickup / minimap ...");
    let (capture_width, buttons) =
        wait_for_buttons(window, button_finder, &["attack", "pickup", "minimap"]);
    println!("✅ [启动自检] 三个常驻按钮均已识别到，缓存坐标");
    state::insert_buttons(buttons.values().cloned().collect());

    let minimap = buttons.get("minimap").expect("已确认存在于本轮结果里");
    if let Err(e) = actions::click_button(enigo, window, capture_width, minimap) {
        alarm::alarm_and_halt(&format!("点击 minimap 失败: {}", e));
    }
    println!("🖱️  已点击 minimap，等待地图打开...");
    thread::sleep(Duration::from_millis(500));

    println!("🔧 [启动自检] 验证 close 按钮 ...");
    let (capture_width2, buttons2) = wait_for_buttons(window, button_finder, &["close"]);
    println!("✅ [启动自检] close 按钮已识别到，点击链路验证通过，缓存坐标");
    state::insert_buttons(buttons2.values().cloned().collect());

    let close = buttons2.get("close").expect("已确认存在于本轮结果里");
    if let Err(e) = actions::click_button(enigo, window, capture_width2, close) {
        alarm::alarm_and_halt(&format!("点击 close 失败: {}", e));
    }
    println!("🖱️  已点击 close，地图已关闭，恢复正常画面");

    println!("🎉 [启动自检] 全部通过，4 个按钮坐标已缓存，进入正式运行");
}

fn capture_and_match(
    window: &Window,
    button_finder: &ButtonFinder,
) -> Result<(u32, Vec<ButtonInfo>), String> {
    let (raw, width, height) = capture_window(window).ok_or("窗口截图失败（窗口可能已关闭）")?;
    let gray = rgba_to_gray_mat(&raw, width, height).map_err(|e| format!("转灰度图失败: {}", e))?;
    let buttons = button_finder
        .find_buttons(&gray, width, MATCH_THRESHOLD)
        .map_err(|e| format!("模板匹配失败: {}", e))?;
    Ok((width, buttons))
}
/// 反复截图匹配，直到 `names` 里列的按钮全部同时出现在同一帧里，
/// 或者等太久——等太久就直接报警并卡死，不会返回。
///
/// 每隔 1 秒打印一次当前进度：截图/匹配是否报错、这一轮识别到了
/// 哪些按钮、还缺哪几个——不再默默重试到超时才让人摸不着头脑。
fn wait_for_buttons(
    window: &Window,
    button_finder: &ButtonFinder,
    names: &[&str],
) -> (u32, HashMap<String, ButtonInfo>) {
    let start = Instant::now();
    let mut last_log = Instant::now() - Duration::from_secs(999); // 让第一轮就打印一次

    loop {
        match capture_and_match(window, button_finder) {
            Ok((width, buttons)) => {
                let map: HashMap<String, ButtonInfo> =
                    buttons.into_iter().map(|b| (b.name.clone(), b)).collect();

                if names.iter().all(|n| map.contains_key(*n)) {
                    return (width, map);
                }

                if last_log.elapsed() >= Duration::from_secs(1) {
                    let missing: Vec<&str> = names
                        .iter()
                        .filter(|n| !map.contains_key(**n))
                        .copied()
                        .collect();
                    println!(
                        "   ⏳ 等待中... 已过 {:.1}s，缺少: {:?}（本轮识别到: {:?}）",
                        start.elapsed().as_secs_f32(),
                        missing,
                        map.keys().collect::<Vec<_>>()
                    );
                    last_log = Instant::now();
                }
            }
            Err(e) => {
                if last_log.elapsed() >= Duration::from_secs(1) {
                    println!(
                        "   ⏳ 等待中... 已过 {:.1}s，本轮截图/匹配失败: {}",
                        start.elapsed().as_secs_f32(),
                        e
                    );
                    last_log = Instant::now();
                }
            }
        }

        if start.elapsed() > TIMEOUT {
            alarm::alarm_and_halt(&format!(
                "等待按钮 {:?} 超时（超过 {} 秒还没同时识别到）",
                names,
                TIMEOUT.as_secs()
            ));
        }

        thread::sleep(POLL_INTERVAL);
    }
}
