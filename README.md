posix_mq
========

This is a simple, relatively high-level library for the POSIX [message queue API][]. It wraps the lower-level API in a
simpler interface with more robust error handling.

Usage example:

```rust
// Values that need to undergo validation are wrapped in safe types:
let name = Name::new("/test-queue").unwrap();

// Queue creation with system defaults is simple:
let queue = Queue::open_or_create(name).expect("Opening queue failed");

// Sending a message:
let message = Message {
  data: "test-message".as_bytes().to_vec(),
  priority: 0,
};
queue.send(&message).expect("message sending failed");

// ... and receiving it!
let result = queue.receive().expect("message receiving failed");
```

[message queue API]: https://linux.die.net/man/7/mq_overview
