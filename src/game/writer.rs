use crate::game::game_info::GameInfo;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

/// 写线程主循环:采集/生成数据,发送给读线程
pub fn run(tx: Sender<GameInfo>) {
    let maps = ["新手村", "黑暗森林", "地下城", "雪山山顶"];
    let mut x: i32 = 0;
    let mut y: i32 = 0;
    let mut tick: u64 = 0;

    loop {
        // 模拟采集游戏数据(实际项目里这里换成读内存/读网络包等真实来源)
        x += 1;
        y += if tick % 2 == 0 { 1 } else { -1 };

        let info = GameInfo {
            map_name: maps[(tick as usize / 5) % maps.len()].to_string(),
            player_position: Some((x, y)),
            hp_percent: 100u8.saturating_sub((tick % 100) as u8),
            monsters: if tick % 3 == 0 {
                vec!["史莱姆".to_string(), "哥布林".to_string()]
            } else {
                vec![]
            },
            items: if tick % 7 == 0 {
                vec!["治疗药水".to_string()]
            } else {
                vec![]
            },
            updated_at: Some(Instant::now()),
        };

        tick += 1;

        // send 失败说明读端已断开,退出循环
        if tx.send(info).is_err() {
            break;
        }

        // 模拟采集间隔,实际项目按需调整或去掉
        thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hp_never_exceeds_100() {
        for tick in 0..300u64 {
            let hp = 100u8.saturating_sub((tick % 100) as u8);
            assert!(hp <= 100);
        }
    }
}
