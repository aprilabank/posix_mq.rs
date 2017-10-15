use super::*;

#[test]
fn test_open_delete() {
    // Simple test with default queue settings
    let name = Name::new("/test-queue").unwrap();
    let queue = Queue::open_or_create(name)
        .expect("Opening queue failed");

    let message = Message {
        data: "test-message".as_bytes().to_vec(),
        priority: 0,
    };

    queue.send(&message).expect("message sending failed");

    let result = queue.receive().expect("message receiving failed");

    assert_eq!(message, result);

    queue.delete();
}
