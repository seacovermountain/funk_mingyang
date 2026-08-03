mod game;

use enigo::{Enigo, Settings};
use game::app_config::AppConfig;
use game::button_finder::ButtonFinder;
use game::ocr::TextOcrRecognizer;
use game::position_reader;
use std::sync::mpsc;
use std::thread;

const WINDOW_TITLE: &str = "24luling";

fn main() {
    game::quit_game_bot::QuitWatchdog::start_async_loop();

    let window = game::window_finder::require_game_window(WINDOW_TITLE);

    // Enigo 全局只创建这一次，后面所有点击动作都传引用复用。
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

    let (tx, rx) = mpsc::channel();

    // 读线程：独立后台线程运行，只负责从 channel 收数据、打印，
    // 不涉及截图/OCR 这些东西，放到子线程里没有跨线程 Send 的顾虑。
    thread::spawn(move || game::reader::run(rx));

    // 写线程：截图 -> 识别 -> 通过 channel 发给读线程。留在主线程里跑
    // （阻塞到程序退出），避免 Window / TextOcrRecognizer 这些类型
    // 要不要跨线程 Send 的不确定性。
    game::writer::run(window, ocr_recognizer, app_config, digit_templates, tx);
}
