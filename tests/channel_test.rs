// tests/channel_test.rs
use std::sync::mpsc;
use std::thread;

#[test]
fn no_message_lost() {
    let (tx, rx) = mpsc::channel::<u32>();

    let writer = thread::spawn(move || {
        for i in 0..1000 {
            tx.send(i).unwrap();
        }
    });

    let reader = thread::spawn(move || {
        let mut received = Vec::new();
        while let Ok(v) = rx.recv() {
            received.push(v);
        }
        received
    });

    writer.join().unwrap();
    let received = reader.join().unwrap();

    // 验证:数量对、顺序对,一条不少
    assert_eq!(received.len(), 1000);
    assert_eq!(received, (0..1000).collect::<Vec<u32>>());
}
