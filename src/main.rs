mod game;
use std::sync::mpsc;
use std::thread;

fn main() {
    game::quit_game_bot::QuitWatchdog::start_async_loop();

    match game::window_finder::find_game_window("24luling") {
        Some(w) => println!("✅ 找到窗口: {:?}", w.title()),
        None => println!("❌ 没找到窗口"),
    }

    // 单生产者单消费者 channel
    let (tx, rx) = mpsc::channel::<game::game_info::GameInfo>();

    // 写线程(生产者)
    let writer_handle = thread::spawn(move || {
        game::writer::run(tx);
    });

    // 读线程(消费者)
    let reader_handle = thread::spawn(move || {
        game::reader::run(rx);
    });

    writer_handle.join().unwrap();
    reader_handle.join().unwrap();

    println!("程序结束");
}
