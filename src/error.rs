use nix;
use std::error;
use std::fmt;

/// This module implements a simple error type to match the errors that can be thrown from the C
/// functions as well as some extra errors resulting from internal validations.
///
/// As this crate exposes an opinionated API to the POSIX queues certain errors have been
/// ignored:
///
/// * ETIMEDOUT: The low-level timed functions are not exported and this error can not occur.
/// * EAGAIN: Non-blocking queue calls are not supported.
/// * EINVAL: Same reason as ETIMEDOUT
/// * EMSGSIZE: The message size is immutable after queue creation and this crate checks it.
/// * ENAMETOOLONG: This crate performs name validation
///
/// If an unexpected error is encountered it will be wrapped appropriately and should be reported
/// as a bug on https://github.com/aprilabank/posix_mq.rs

#[derive(Debug)]
pub enum Error {
    // These errors match what is described in the man pages (from mq_overview(7) onwards).
    PermissionDenied(),
    InvalidQueueDescriptor(),
    QueueCallInterrupted(),
    QueueAlreadyExists(),
    QueueNotFound(),
    InsufficientMemory(),
    InsufficientSpace(),

    // These two are (hopefully) unlikely in modern systems
    ProcessFileDescriptorLimitReached(),
    SystemFileDescriptorLimitReached(),

    // If an unhandled / unknown / unexpected error occurs this error will be used.
    // In those cases bug reports would be welcome!
    UnknownForeignError(nix::Errno),

    // Some other unexpected / unknown error occured. This is probably an error from
    // the nix crate. Bug reports also welcome for this!
    UnknownInternalError(Option<nix::Error>),
}

impl error::Error for Error {
    fn description(&self) -> &str {
        use Error::*;
        match *self {
            PermissionDenied() => "permission to the specified queue was denied",
            InvalidQueueDescriptor() => "the internal queue descriptor was invalid",
            QueueCallInterrupted() => "queue method interrupted by signal",
            QueueAlreadyExists() => "the specified queue already exists",
            QueueNotFound() => "the specified queue could not be found",
            InsufficientMemory() => "insufficient memory to call queue method",
            InsufficientSpace() => "insufficient space to call queue method",
            ProcessFileDescriptorLimitReached() => "max. number of process file descriptors reached",
            SystemFileDescriptorLimitReached() => "max. number of system file descriptors reached",
            UnknownForeignError(_) => "unknown foreign error occured: please report a bug!",
            UnknownInternalError(_) => "unknown internal error occured: please report a bug!",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Explicitly import this to gain access to Error::description()
        use std::error::Error;
        f.write_str(self.description())
    }
}

/// This from implementation is used to translate errors from the lower-level
/// C-calls into sensible Rust errors.
impl From<nix::Error> for Error {
    fn from(e: nix::Error) -> Self {
        match e {
            nix::Error::Sys(e) => match_errno(e),
            _ => Error::UnknownInternalError(Some(e)),
        }
    }
}

fn match_errno(err: nix::Errno) -> Error {
    use nix::errno::*;

    match err {
        EACCES => Error::PermissionDenied(),
        EBADF  => Error::InvalidQueueDescriptor(),
        EINTR  => Error::QueueCallInterrupted(),
        EEXIST => Error::QueueAlreadyExists(),
        EMFILE => Error::ProcessFileDescriptorLimitReached(),
        ENFILE => Error::SystemFileDescriptorLimitReached(),
        ENOENT => Error::QueueNotFound(),
        ENOMEM => Error::InsufficientMemory(),
        ENOSPC => Error::InsufficientSpace(),
        _      => Error::UnknownForeignError(err),
    }
}
