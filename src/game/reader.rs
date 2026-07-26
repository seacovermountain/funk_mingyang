use crate::game::game_info::GameInfo;
use std::sync::mpsc::Receiver;

/// 读线程主循环:不漏掉任何一次更新,依次处理
pub fn run(rx: Receiver<GameInfo>) {
    // rx.recv() 阻塞等待,直到收到数据或 channel 关闭(写端 drop)
    while let Ok(info) = rx.recv() {
        // TODO: 在这里处理每一条 GameInfo(打印 / 存储 / 触发逻辑判断等)
        handle_info(info);
    }
    // channel 已关闭,读线程自然结束
}

fn handle_info(info: GameInfo) {
    // TODO: 具体处理逻辑,先占位打印
    println!("{:?}", info);
}
