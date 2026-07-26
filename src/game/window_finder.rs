use std::thread::sleep;
use std::time::Duration;
use xcap::Window;

/// 根据标题关键字查找一个"活动中"的窗口(大小写不敏感、模糊匹配)。
///
/// 返回 None 的情况:
/// - 系统里没有任何窗口标题包含该关键字
/// - 匹配到的窗口都处于最小化状态(视为不可用,跳过)
/// - 底层 API 调用失败(权限不足 / 平台异常等),Window::all() 返回 Err
pub fn find_game_window(title: &str) -> Option<Window> {
    let target_title = title.to_lowercase();

    Window::all().ok()?.into_iter().find(|w| {
        let w_title = w.title().unwrap_or_default().to_lowercase();
        // 拿不到最小化状态时保守处理,当成"不可用",避免误判
        let minimized = w.is_minimized().unwrap_or(true);

        w_title.contains(&target_title) && !minimized
    })
}

/// 阻塞式查找游戏窗口,找不到就重试,重试耗尽直接终止进程。
///
/// 适合在程序启动阶段调用:游戏窗口是后续所有操作的前提,
/// 如果长时间找不到,继续跑下去也没有意义,不如尽早失败。
///
/// # Panics / 进程退出
/// 连续 `max_attempts` 次都没找到窗口时,会打印错误信息并调用 `std::process::exit(1)`。
pub fn require_game_window(title: &str) -> Window {
    require_game_window_with(title, 5, Duration::from_secs(2))
}

/// `require_game_window` 的可配置版本,方便调整重试次数和间隔(比如写测试、或者不同场景下用不同策略)。
pub fn require_game_window_with(title: &str, max_attempts: u32, interval: Duration) -> Window {
    println!("🔄 正在全系统检索包含关键字 [{}] 的活动游戏窗口...", title);

    for attempt in 1..=max_attempts {
        if let Some(window) = find_game_window(title) {
            println!(
                "🎯 [窗口锁定成功] 标题: \"{}\" | 坐标: ({}, {}) | 分辨率: {}x{}",
                window.title().unwrap_or_default(),
                window.x().unwrap_or(0),
                window.y().unwrap_or(0),
                window.width().unwrap_or(0),
                window.height().unwrap_or(0),
            );
            return window;
        }

        println!(
            "⚠️  第 {} / {} 次尝试:未检测到游戏窗口,{:?} 后重试...",
            attempt, max_attempts, interval
        );
        sleep(interval);
    }

    println!(
        "\n❌ [严重错误] 连续 {} 次未检测到游戏窗口,程序失去运行基础,强制闪退!",
        max_attempts
    );
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    // 依赖当前机器真实的窗口环境,CI 里不一定有窗口,标记 ignore,
    // 本地手动跑 `cargo test -- --ignored` 验证。
    #[test]
    #[ignore]
    fn returns_none_for_nonexistent_title() {
        let result = find_game_window("这个标题_绝对不存在_xyz123");
        assert!(result.is_none());
    }

    #[test]
    #[ignore]
    fn require_with_zero_attempts_never_finds_and_exits() {
        // 注意: 这个测试会真的调用 process::exit,不要在正常测试流程里跑,
        // 仅作为行为说明放在这里,实际验证建议手动执行观察输出。
        let _ = require_game_window_with("不存在的窗口", 1, Duration::from_millis(100));
    }
}
