// src/game/reader.rs
//
// 读线程：通过 channel 接收写线程识别好的 GameInfo，一收到就立刻处理。
// 目前只是打印出来，后续要接业务逻辑（比如自动战斗决策）就在
// print_snapshot 这个位置往下扩展；如果业务逻辑需要跨线程共享某些
// 派生状态，在这里自己开一个全局变量存就行，不需要再回头改写线程。

use crate::game::game_info::GameInfo;
use crate::game::state;
use std::sync::mpsc::Receiver;

pub fn run(rx: Receiver<GameInfo>) {
    // rx.recv() 会阻塞在这里，写线程一 send() 就立刻醒过来处理，
    // 不用像轮询那样瞎猜间隔，也不会把同一份没变化的数据重复打印。
    // 写线程退出（Sender 被 drop）时，for 循环自然结束，读线程也退出。
    for info in rx {
        print_snapshot(&info);
        handle_business_logic(&info);
    }
    println!("⚠️  [读线程] 写线程已断开，读线程退出");
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

/// 业务逻辑占位示例：演示"要做一件会让截图暂时失真的操作（比如打开
/// 大地图）之前，先关掉写开关；操作做完、画面恢复正常了，再打开"
/// 这个模式该怎么写。
///
/// 下面这个触发条件（血量低于 20% 就点开大地图）纯粹是随手写的占位
/// 例子，不是真实业务需求——真正要在什么条件下做什么操作，换成你
/// 自己的判断逻辑就行，关键是 disable_writer() / enable_writer()
/// 要成对出现，而且要包住"画面会变的操作"这一段，不多不少。
fn handle_business_logic(info: &GameInfo) {
    if info.hp_percent < 20 {
        // 1. 先关闭写开关：写线程下一轮循环检查到开关关了，就会跳过
        //    截图识别，只空转等待，不会把"大地图界面"误当成正常游戏
        //    画面去识别。
        state::disable_writer();
        println!("   ⏸️  [读线程] 血量过低，暂停写线程，准备打开大地图...");

        // 2. 这里放真正的业务操作，比如：点击大地图按钮、等待地图
        //    渲染出来、读取地图信息、点击关闭按钮、等画面变回正常。
        //    这一步耗时多久、中间要不要重试，都不影响写线程——反正
        //    写线程这段时间只是在空转等待，不会去截图。

        // 3. 操作做完、画面已经恢复正常游戏界面了，再重新打开写开关，
        //    写线程从下一轮循环开始就会恢复正常截图识别。
        state::enable_writer();
        println!("   ▶️  [读线程] 业务逻辑处理完，恢复写线程");
    }
}
