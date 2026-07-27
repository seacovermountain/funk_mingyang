// src/game/alarm.rs
//
// 报警铃声：音频文件用 include_bytes! 直接打包进可执行文件本身，
// 不依赖外部文件路径、不用区分 Windows/macOS 分别调用系统命令——
// 用 rodio（底层基于 cpal），跨平台通用。
//
// 用途：自检失败、需要人工介入的场景。程序在这里彻底停下来，循环
// 报警，直到人工用 ESC 长按退出（复用已有的退出监听），不自动重试。

use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;
use std::thread;
use std::time::Duration;

/// 报警音效，编译时直接打包进可执行文件里。
static ALARM_SOUND: &[u8] = include_bytes!("../../assets/alarm.wav");

/// 报警并且不停循环播放，直到进程被外部终止（比如 ESC 长按退出）。
/// 函数不会返回——调用它就意味着"程序不该再继续往下跑了"。
pub fn alarm_and_halt(reason: &str) -> ! {
    println!("🚨🚨🚨 自检失败: {} —— 需要人工介入，程序已停止", reason);

    let (_stream, stream_handle) =
        OutputStream::try_default().expect("❌ 打不开系统音频输出设备，报警声音放不出来");

    loop {
        match Decoder::new(Cursor::new(ALARM_SOUND)) {
            Ok(source) => {
                if let Ok(sink) = Sink::try_new(&stream_handle) {
                    sink.append(source);
                    sink.sleep_until_end();
                }
            }
            Err(e) => println!("⚠️  报警音效解码失败: {}", e),
        }
        thread::sleep(Duration::from_millis(500));
    }
}
