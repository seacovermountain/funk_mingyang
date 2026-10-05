// src/game/path_recorder.rs
//
// 🎥 巡逻路径录制：按 F9 开始/停止录制。录制期间，reader.rs 会按
// "跟上一个记录点的距离够远了" 为条件，把角色当前坐标记下来；
// 停止录制时，reader.rs 会把记下来的这一串坐标，格式化成能直接
// 复制粘贴进 config.toml 的 [[patrol]] 配置块，打印到终端，并顺手
// 存成一个文件方便复制粘贴。
//
// 这个模块本身只负责"监听热键、切换开关"，不知道当前地图/坐标是
// 什么——那些信息只有 reader.rs 处理每一帧 GameInfo 时才有，所以
// "真正采样 + 停止时落盘"的逻辑放在 reader.rs 里，靠"这一帧 vs 上
// 一帧的录制开关状态发生了变化"来判断"刚开始录制"/"刚停止录制"。

use crate::game::state;
use device_query::{DeviceQuery, DeviceState, Keycode};
use std::thread;
use std::time::Duration;

pub struct PathRecorderHotkey;

impl PathRecorderHotkey {
    /// 🚀 启动热键监听守护线程：按一下 F9 切换"录制中/未录制"。
    /// 用"按下沿"触发(按住不放只切换一次)，不是每次轮询到按下就
    /// 切换一次状态，不然按一下键会因为轮询间隔太快而反复横跳。
    pub fn start_async_loop() {
        thread::spawn(move || {
            let device_state = DeviceState::new();
            let mut was_pressed = false;

            loop {
                let keys: Vec<Keycode> = device_state.get_keys();
                let pressed = keys.contains(&Keycode::F9);

                if pressed && !was_pressed {
                    let now_recording = state::toggle_recording();
                    if now_recording {
                        println!(
                            "🎥 [路径录制] 开始录制，去游戏里走你想要的巡逻路线吧（再按一次 F9 停止）"
                        );
                    } else {
                        println!("🎥 [路径录制] 停止录制，稍后会打印/存盘录到的坐标");
                    }
                }
                was_pressed = pressed;

                thread::sleep(Duration::from_millis(50));
            }
        });
    }
}
