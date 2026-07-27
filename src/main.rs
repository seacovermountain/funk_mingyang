mod game;

use enigo::{Enigo, Settings};
use game::app_config::AppConfig;
use game::button_finder::ButtonFinder;
use game::ocr::text_ocr::TextOcrRecognizer;

const WINDOW_TITLE: &str = "24luling";
const BUTTON_CONFIG_PATH: &str = "assets/buttons/buttons.toml";
const APP_CONFIG_PATH: &str = "config.toml";
const OCR_DET_MODEL: &str = "models/PP-OCRv6_medium_det.mnn";
const OCR_REC_MODEL: &str = "models/PP-OCRv6_medium_rec.mnn";
const OCR_KEYS_FILE: &str = "models/ppocr_keys_v6_medium.txt";

fn main() {
    game::quit_game_bot::QuitWatchdog::start_async_loop();

    let window = game::window_finder::require_game_window(WINDOW_TITLE);

    // Enigo 全局只创建这一次，后面所有点击动作都传引用复用。
    let mut enigo = Enigo::new(&Settings::default()).expect("❌ 初始化鼠标控制失败");

    game::actions::activate_window(&mut enigo, &window).expect("❌ 点击窗口中心失败");

    let button_finder = ButtonFinder::load(BUTTON_CONFIG_PATH)
        .expect("❌ 按钮模板加载失败，检查 assets/buttons/buttons.toml 和对应的图片文件");

    let app_config = AppConfig::load(APP_CONFIG_PATH)
        .expect("❌ 读取 config.toml 失败，检查怪物/物品白名单配置");

    let ocr_recognizer = TextOcrRecognizer::new(OCR_DET_MODEL, OCR_REC_MODEL, OCR_KEYS_FILE)
        .expect("❌ OCR 模型加载失败，检查 models/ 目录下的模型文件路径和文件名");

    game::startup_check::run(&window, &button_finder, &mut enigo);
    game::bot::run(window, ocr_recognizer, app_config);
}
