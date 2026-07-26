// src/game/reader.rs
//
// 读线程主循环：接收写线程识别到的按钮快照，打印出来验证链路是否跑通。
// 后续要做"点击按钮"之类的动作判断，也是在这里（或者从这里派生新逻辑）。

use crate::game::state::ButtonSnapshot;
use std::sync::mpsc::Receiver;

pub fn run(rx: Receiver<ButtonSnapshot>) {
    // rx.recv() 阻塞等待，直到收到数据或 channel 关闭（写端 drop）
    while let Ok(snapshot) = rx.recv() {
        handle_snapshot(snapshot);
    }
    // channel 已关闭，读线程自然结束
}

fn handle_snapshot(snapshot: ButtonSnapshot) {
    if snapshot.buttons.is_empty() {
        println!("🔍 本轮未识别到任何按钮");
        return;
    }

    println!("🔍 本轮识别到 {} 个按钮：", snapshot.buttons.len());
    for b in &snapshot.buttons {
        println!(
            "   - {:<10} 坐标 ({:>4}, {:>4})  尺寸 {:>3}x{:<3}  置信度 {:.2}",
            b.name, b.x, b.y, b.width, b.height, b.confidence
        );
    }
}
