mod game;

use enigo::{Enigo, Settings};
use game::app_config::AppConfig;
use game::button_finder::ButtonFinder;
use game::ocr::TextOcrRecognizer;
use game::position_reader;
use std::thread;

const WINDOW_TITLE: &str = "24luling";

fn main() {
    game::quit_game_bot::QuitWatchdog::start_async_loop();
    game::path_recorder::PathRecorderHotkey::start_async_loop();

    let window = game::window_finder::require_game_window(WINDOW_TITLE);

    // Enigo 全局只创建这一次，后面所有点击动作都传引用复用。
    // 🧭 Enigo 持有底层系统句柄，稳妥起见全程只在"主线程"里创建和
    // 使用，不跨线程传递——这也是为什么写线程（截图识别，不需要
    // 点击）被挪去了子线程，而读线程（需要点击拾取/攻击/寻路）留在
    // 主线程里跑，反而是原来"写线程占用主线程"的安排倒过来了。
    let mut enigo = Enigo::new(&Settings::default()).expect("❌ 初始化鼠标控制失败");

    game::actions::activate_window(&mut enigo, &window).expect("❌ 点击窗口中心失败");

    // 按钮图标、config.toml、OCR 模型、坐标数字模板现在都在编译时嵌入
    // 进了可执行文件里（见各自模块里的 include_bytes!/include_str!），
    // 这几个 ::load() 不再需要传路径，部署只需要这一个可执行文件。
    let button_finder = ButtonFinder::load().expect("❌ 按钮模板加载失败（内置模板解码出错）");

    let app_config = AppConfig::load().expect("❌ 内置配置解析失败，检查 config.toml 的格式");

    let ocr_recognizer = TextOcrRecognizer::new().expect("❌ OCR 模型加载失败（内置模型数据出错）");

    let digit_templates = position_reader::load_digit_templates()
        .expect("❌ 坐标数字模板加载失败（内置模板解码出错）");

    game::startup_check::run(&window, &button_finder, &mut enigo);

    // 🔄 写线程和读线程不再通过 channel 传递数据，改成都读写
    // state::LATEST_GAME_INFO 这个共享状态——写线程只管尽量快地持续
    // 更新它，读线程按自己固定的节奏(reader.rs 里的 ACTION_INTERVAL)
    // 去读最新值做决策，两边互不阻塞，不会出现"读线程被写线程的识别
    // 速度拖慢"的问题。
    //
    // 写线程：截图 -> 识别 -> 写共享状态。挪到子线程里跑，因为它不需要
    // 碰 Enigo，`Window` 又是 Clone 的，各自留一份互不影响。AppConfig
    // 也是 Clone 的，写线程（怪物/物品白名单匹配）和读线程（巡逻路线
    // 查表）各拿一份。
    let writer_window = window.clone();
    let writer_app_config = app_config.clone();
    thread::spawn(move || {
        game::writer::run(
            writer_window,
            ocr_recognizer,
            writer_app_config,
            digit_templates,
        )
    });

    // 读线程 + 决策 + 点击动作：留在主线程里跑（阻塞到程序退出），
    // 持有 Enigo 和 Window，负责"拾取 > 打怪 > 寻路"整套状态机。
    game::reader::run(enigo, window, app_config);
}
