extern crate nix;
extern crate libc;

use error::Error;
use libc::mqd_t;
use nix::mqueue;
use nix::sys::stat;
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::string::ToString;
use std::ops::Drop;

mod error;

#[cfg(test)]
mod tests;

/*
TODO:

* what happens if permissions change after FD was opened?
* drop dependency on nix crate?

*/

/// Wrapper type for queue names that performs basic validation of queue names before calling
/// out to C code.
#[derive(Debug)]
pub struct Name(CString);

impl Name {
    pub fn new<S: ToString>(s: S) -> Result<Self, Error> {
        let string = s.to_string();

        if !string.starts_with('/') {
            return Err(Error::InvalidQueueName("Queue name must start with '/'"));
        }

        // The C library has a special error return for this case, so I assume people must actually
        // have tried just using '/' as a queue name.
        if string.len() == 1 {
            return Err(Error::InvalidQueueName(
                "Queue name must be a slash followed by one or more characters"
            ));
        }

        if string.len() > 255 {
            return Err(Error::InvalidQueueName("Queue name must not exceed 255 characters"));
        }

        if string.matches('/').count() > 1 {
            return Err(Error::InvalidQueueName("Queue name can not contain more than one slash"));
        }

        // TODO: What error is being thrown away here? Is it possible?
        Ok(Name(CString::new(string).unwrap()))
    }
}

#[derive(Debug, PartialEq)]
pub struct Message {
    pub data: Vec<u8>,
    pub priority: u32,
}

/// Represents an open queue descriptor to a POSIX message queue. This carries information
/// about the queue's limitations (i.e. maximum message size and maximum message count).
#[derive(Debug)]
pub struct Queue {
    name: Name,

    /// Internal file/queue descriptor.
    queue_descriptor: mqd_t,

    /// Maximum number of pending messages in this queue.
    max_pending: i64,

    /// Maximum size of this queue.
    max_size: usize,
}

impl Queue {
    /// Creates a new queue and fails if it already exists.
    /// By default the queue will be read/writable by the current user with no access for other
    /// users.
    /// Linux users can change this setting themselves by modifying the queue file in /dev/mqueue.
    pub fn create(name: Name, max_pending: i64, max_size: i64) -> Result<Queue, Error> {
        if max_pending > read_i64_from_file(MSG_MAX)? {
            return Err(Error::MaximumMessageCountExceeded());
        }

        if max_size > read_i64_from_file(MSGSIZE_MAX)? {
            return Err(Error::MaximumMessageSizeExceeded());
        }

        let oflags = {
            let mut flags = mqueue::MQ_OFlag::empty();
            // Put queue in r/w mode
            flags.toggle(mqueue::O_RDWR);
            // Enable queue creation
            flags.toggle(mqueue::O_CREAT);
            // Fail if queue exists already
            flags.toggle(mqueue::O_EXCL);
            flags
        };

        let attr = mqueue::MqAttr::new(
            0, max_pending, max_size, 0
        );

        let queue_descriptor = mqueue::mq_open(
            &name.0,
            oflags,
            default_mode(),
            Some(&attr),
        )?;

        Ok(Queue {
            name,
            queue_descriptor,
            max_pending,
            max_size: max_size as usize,
        })
    }

    /// Opens an existing queue.
    pub fn open(name: Name) -> Result<Queue, Error> {
        // No extra flags need to be constructed as the default is to open and fail if the
        // queue does not exist yet - which is what we want here.
        let oflags = mqueue::O_RDWR;
        let queue_descriptor = mqueue::mq_open(
            &name.0,
            oflags,
            default_mode(),
            None,
        )?;

        let attr = mq_getattr(queue_descriptor)?;

        Ok(Queue {
            name,
            queue_descriptor,
            max_pending: attr.mq_maxmsg,
            max_size: attr.mq_msgsize as usize,
        })
    }

    /// Opens an existing queue or creates a new queue with the OS default settings.
    pub fn open_or_create(name: Name) -> Result<Queue, Error> {
        let oflags = {
            let mut flags = mqueue::MQ_OFlag::empty();
            // Put queue in r/w mode
            flags.toggle(mqueue::O_RDWR);
            // Enable queue creation
            flags.toggle(mqueue::O_CREAT);
            flags
        };

        let default_pending = read_i64_from_file(MSG_DEFAULT)?;
        let default_size = read_i64_from_file(MSGSIZE_DEFAULT)?;
        let attr = mqueue::MqAttr::new(
            0, default_pending, default_size, 0
        );

        let queue_descriptor = mqueue::mq_open(
            &name.0,
            oflags,
            default_mode(),
            Some(&attr),
        )?;

        let actual_attr = mq_getattr(queue_descriptor)?;

        Ok(Queue {
            name,
            queue_descriptor,
            max_pending: actual_attr.mq_maxmsg,
            max_size: actual_attr.mq_msgsize as usize,
        })
    }

    /// Delete a message queue from the system. This method will make the queue unavailable for
    /// other processes after their current queue descriptors have been closed.
    pub fn delete(self) -> Result<(), Error> {
        mqueue::mq_unlink(&self.name.0)?;
        drop(self);
        Ok(())
    }

    /// Send a message to the message queue.
    /// If the queue is full this call will block until a message has been consumed.
    pub fn send(&self, msg: &Message) -> Result<(), Error> {
        if msg.data.len() > self.max_size as usize {
            return Err(Error::MessageSizeExceeded());
        }

        mqueue::mq_send(
            self.queue_descriptor,
            msg.data.as_ref(),
            msg.priority,
        ).map_err(|e| e.into())
    }

    /// Receive a message from the message queue.
    /// If the queue is empty this call will block until a message arrives.
    pub fn receive(&self) -> Result<Message, Error> {
        let mut data: Vec<u8> = vec![0; self.max_size as usize];
        let mut priority: u32 = 0;

        let msg_size = mqueue::mq_receive(
            self.queue_descriptor,
            data.as_mut(),
            &mut priority,
        )?;

        data.truncate(msg_size);
        Ok(Message { data, priority })
    }

    pub fn max_pending(&self) -> i64 {
        self.max_pending
    }

    pub fn max_size(&self) -> usize {
        self.max_size
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        // Attempt to close the queue descriptor and discard any possible errors.
        // The only error thrown in the C-code is EINVAL, which would mean that the
        // descriptor has already been closed.
        mqueue::mq_close(self.queue_descriptor).ok();
    }
}

// Creates the default queue mode (0600).
fn default_mode() -> stat::Mode {
    let mut mode = stat::Mode::empty();
    mode.toggle(stat::S_IRUSR);
    mode.toggle(stat::S_IWUSR);
    mode
}

/// This file defines the default number of maximum pending messages in a queue.
const MSG_DEFAULT: &'static str = "/proc/sys/fs/mqueue/msg_default";

/// This file defines the system maximum number of pending messages in a queue.
const MSG_MAX: &'static str = "/proc/sys/fs/mqueue/msg_max";

/// This file defines the default maximum size of messages in a queue.
const MSGSIZE_DEFAULT: &'static str = "/proc/sys/fs/mqueue/msgsize_default";

/// This file defines the system maximum size for messages in a queue.
const MSGSIZE_MAX: &'static str = "/proc/sys/fs/mqueue/msgsize_max";

/// This method is used in combination with the above constants to find system limits.
fn read_i64_from_file(name: &str) -> Result<i64, Error> {
    let mut file = File::open(name.to_string())?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content.trim().parse()?)
}

/// The mq_getattr implementation in the nix crate hides the maximum message size and count, which
/// is very impractical.
/// To work around it, this method calls the C-function directly.
fn mq_getattr(mqd: mqd_t) -> Result<libc::mq_attr, Error> {
    use std::mem;
    let mut attr = unsafe { mem::uninitialized::<libc::mq_attr>() };
    let res = unsafe { libc::mq_getattr(mqd, &mut attr) };
    nix::Errno::result(res)
        .map(|_| attr)
        .map_err(|e| e.into())
}
