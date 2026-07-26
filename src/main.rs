mod game;

use game::button_finder::ButtonFinder;

const WINDOW_TITLE: &str = "24luling";
const BUTTON_CONFIG_PATH: &str = "assets/buttons/buttons.toml";

fn main() {
    game::quit_game_bot::QuitWatchdog::start_async_loop();

    let window = game::window_finder::require_game_window(WINDOW_TITLE);

    let button_finder = ButtonFinder::load(BUTTON_CONFIG_PATH)
        .expect("❌ 按钮模板加载失败，检查 assets/buttons/buttons.toml 和对应的图片文件");

    game::bot::run(window, button_finder);
}
