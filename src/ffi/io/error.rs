use std::io::ErrorKind;

/// [std::io::Error] and [std::io::ErrorKind], but with `uniffi` support.
#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum IoError {
    /// An entity was not found, often a file.
    #[error("entity not found")]
    NotFound,
    /// The operation lacked the necessary privileges to complete.
    #[error("permission defined")]
    PermissionDenied,
    /// The connection was refused by the remote server.
    #[error("connection refused")]
    ConnectionRefused,
    /// The connection was reset by the remote server.
    #[error("connection reset")]
    ConnectionReset,
    /// The remote host is not reachable.
    #[error("host unreachable")]
    HostUnreachable,
    /// The network containing the remote host is not reachable.
    #[error("network unreachable")]
    NetworkUnreachable,
    /// The connection was aborted (terminated) by the remote server.
    #[error("connection aborted")]
    ConnectionAborted,
    /// The network operation failed because it was not connected yet.
    #[error("not connected")]
    NotConnected,
    /// A socket address could not be bound because the address is already in
    /// use elsewhere.
    #[error("address in use")]
    AddrInUse,
    /// A nonexistent interface was requested or the requested address was not
    /// local.
    #[error("address not available")]
    AddrNotAvailable,
    /// The system's networking is down.
    #[error("network down")]
    NetworkDown,
    /// The operation failed because a pipe was closed.
    #[error("broken pipe")]
    BrokenPipe,
    /// An entity already exists, often a file.
    #[error("entity already exists")]
    AlreadyExists,
    /// The operation needs to block to complete, but the blocking operation was
    /// requested to not occur.
    #[error("operation would block")]
    WouldBlock,
    /// A filesystem object is, unexpectedly, not a directory.
    ///
    /// For example, a filesystem path was specified where one of the intermediate directory
    /// components was, in fact, a plain file.
    #[error("not a directory")]
    NotADirectory,
    /// The filesystem object is, unexpectedly, a directory.
    ///
    /// A directory was specified when a non-directory was expected.
    #[error("is a directory")]
    IsADirectory,
    /// A non-empty directory was specified where an empty directory was expected.
    #[error("directory not empty")]
    DirectoryNotEmpty,
    /// The filesystem or storage medium is read-only, but a write operation was attempted.
    #[error("read-only filesystem or storage medium")]
    ReadOnlyFilesystem,
    /// Loop in the filesystem or IO subsystem; often, too many levels of symbolic links.
    ///
    /// There was a loop (or excessively long chain) resolving a filesystem object
    /// or file IO object.
    ///
    /// On Unix this is usually the result of a symbolic link loop; or, of exceeding the
    /// system-specific limit on the depth of symlink traversal.
    #[error("filesystem loop or indirection limit (e.g. symlink loop)")]
    FilesystemLoop,
    /// Stale network file handle.
    ///
    /// With some network filesystems, notably NFS, an open file (or directory) can be invalidated
    /// by problems with the network or server.
    #[error("stale network file handle")]
    StaleNetworkFileHandle,
    /// A parameter was incorrect.
    #[error("invalid input parameter")]
    InvalidInput,
    /// Data not valid for the operation were encountered.
    ///
    /// Unlike [`InvalidInput`], this typically means that the operation
    /// parameters were valid, however the error was caused by malformed
    /// input data.
    ///
    /// For example, a function that reads a file into a string will error with
    /// `InvalidData` if the file's contents are not valid UTF-8.
    ///
    /// [`InvalidInput`]: ErrorKind::InvalidInput
    #[error("invalid data")]
    InvalidData,
    /// The I/O operation's timeout expired, causing it to be canceled.
    #[error("timed out")]
    TimedOut,
    /// An error returned when an operation could not be completed because a
    /// call to [`write`] returned [`Ok(0)`].
    ///
    /// This typically means that an operation could only succeed if it wrote a
    /// particular number of bytes but only a smaller number of bytes could be
    /// written.
    ///
    /// [`write`]: std::io::Write::write
    /// [`Ok(0)`]: Ok
    #[error("write zero")]
    WriteZero,
    /// The underlying storage (typically, a filesystem) is full.
    ///
    /// This does not include out of quota errors.
    #[error("no storage space")]
    StorageFull,
    /// Seek on unseekable file.
    ///
    /// Seeking was attempted on an open file handle which is not suitable for seeking - for
    /// example, on Unix, a named pipe opened with `File::open`.
    #[error("seek on unseekable file")]
    NotSeekable,
    /// Filesystem quota was exceeded.
    #[error("filesystem quota exceeded")]
    FilesystemQuotaExceeded,
    /// File larger than allowed or supported.
    ///
    /// This might arise from a hard limit of the underlying filesystem or file access API, or from
    /// an administratively imposed resource limitation.  Simple disk full, and out of quota, have
    /// their own errors.
    #[error("file too large")]
    FileTooLarge,
    /// Resource is busy.
    #[error("resource busy")]
    ResourceBusy,
    /// Executable file is busy.
    ///
    /// An attempt was made to write to a file which is also in use as a running program.  (Not all
    /// operating systems detect this situation.)
    #[error("executable file busy")]
    ExecutableFileBusy,
    /// Deadlock (avoided).
    ///
    /// A file locking operation would result in deadlock.  This situation is typically detected, if
    /// at all, on a best-effort basis.
    #[error("deadlock")]
    Deadlock,
    /// Cross-device or cross-filesystem (hard) link or rename.
    #[error("cross-device link or rename")]
    CrossesDevices,
    /// Too many (hard) links to the same filesystem object.
    ///
    /// The filesystem does not support making so many hardlinks to the same file.
    #[error("too many links")]
    TooManyLinks,
    /// A filename was invalid.
    ///
    /// This error can also cause if it exceeded the filename length limit.
    #[error("invalid filename")]
    InvalidFilename,
    /// Program argument list too long.
    ///
    /// When trying to run an external program, a system or process limit on the size of the
    /// arguments would have been exceeded.
    #[error("argument list too long")]
    ArgumentListTooLong,
    /// This operation was interrupted.
    ///
    /// Interrupted operations can typically be retried.
    #[error("operation interrupted")]
    Interrupted,

    /// This operation is unsupported on this platform.
    ///
    /// This means that the operation can never succeed.
    #[error("unsupported")]
    Unsupported,

    // ErrorKinds which are primarily categorisations for OS error
    // codes should be added above.
    //
    /// An error returned when an operation could not be completed because an
    /// "end of file" was reached prematurely.
    ///
    /// This typically means that an operation could only succeed if it read a
    /// particular number of bytes but only a smaller number of bytes could be
    /// read.
    #[error("unexpected end of file")]
    UnexpectedEof,

    /// An operation could not be completed, because it failed
    /// to allocate enough memory.
    #[error("out of memory")]
    OutOfMemory,

    // "Unusual" error kinds which do not correspond simply to (sets
    // of) OS error codes, should be added just above this comment.
    // `Other` and `Uncategorized` should remain at the end:
    //
    /// A custom error that does not fall under any other I/O error kind.
    ///
    /// This can be used to construct your own [`IoError`]s that do not match any
    /// [`ErrorKind`].
    ///
    /// This [`ErrorKind`] is not used by the standard library.
    ///
    /// Errors from the standard library that do not fall under any of the I/O
    /// error kinds cannot be `match`ed on, and will only match a wildcard (`_`) pattern.
    /// New [`ErrorKind`]s might be added in the future for some of those.
    #[error("other error")]
    Other,

    /// Any I/O error from the standard library that's not part of this list.
    ///
    /// Errors that are `Uncategorized` now may move to a different or a new
    /// [`ErrorKind`] variant in the future. It is not recommended to match
    /// an error against `Uncategorized`; use a wildcard match (`_`) instead.

    #[doc(hidden)]
    #[error("uncategorized error")]
    Uncategorized,
}
impl From<&std::io::Error> for IoError {
    fn from(std_io_error: &std::io::Error) -> Self {
        std_io_error.kind().into()
    }
}

impl From<ErrorKind> for IoError {
    fn from(error_kind: ErrorKind) -> Self {
        match error_kind {
            ErrorKind::NotFound => Self::NotFound,
            ErrorKind::PermissionDenied => Self::PermissionDenied,
            ErrorKind::ConnectionRefused => Self::ConnectionRefused,
            ErrorKind::ConnectionReset => Self::ConnectionReset,
            ErrorKind::HostUnreachable => Self::HostUnreachable,
            ErrorKind::NetworkUnreachable => Self::NetworkUnreachable,
            ErrorKind::ConnectionAborted => Self::ConnectionAborted,
            ErrorKind::NotConnected => Self::NotConnected,
            ErrorKind::AddrInUse => Self::AddrInUse,
            ErrorKind::AddrNotAvailable => Self::AddrNotAvailable,
            ErrorKind::NetworkDown => Self::NetworkDown,
            ErrorKind::BrokenPipe => Self::BrokenPipe,
            ErrorKind::AlreadyExists => Self::AlreadyExists,
            ErrorKind::WouldBlock => Self::WouldBlock,
            ErrorKind::NotADirectory => Self::NotADirectory,
            ErrorKind::IsADirectory => Self::IsADirectory,
            ErrorKind::DirectoryNotEmpty => Self::DirectoryNotEmpty,
            ErrorKind::ReadOnlyFilesystem => Self::ReadOnlyFilesystem,
            ErrorKind::FilesystemLoop => Self::FilesystemLoop,
            ErrorKind::StaleNetworkFileHandle => Self::StaleNetworkFileHandle,
            ErrorKind::InvalidInput => Self::InvalidInput,
            ErrorKind::InvalidData => Self::InvalidData,
            ErrorKind::TimedOut => Self::TimedOut,
            ErrorKind::WriteZero => Self::WriteZero,
            ErrorKind::StorageFull => Self::StorageFull,
            ErrorKind::NotSeekable => Self::NotSeekable,
            ErrorKind::FilesystemQuotaExceeded => Self::FilesystemQuotaExceeded,
            ErrorKind::FileTooLarge => Self::FileTooLarge,
            ErrorKind::ResourceBusy => Self::ResourceBusy,
            ErrorKind::ExecutableFileBusy => Self::ExecutableFileBusy,
            ErrorKind::Deadlock => Self::Deadlock,
            ErrorKind::CrossesDevices => Self::CrossesDevices,
            ErrorKind::TooManyLinks => Self::TooManyLinks,
            ErrorKind::InvalidFilename => Self::InvalidFilename,
            ErrorKind::ArgumentListTooLong => Self::ArgumentListTooLong,
            ErrorKind::Interrupted => Self::Interrupted,
            ErrorKind::Unsupported => Self::Unsupported,
            ErrorKind::UnexpectedEof => Self::UnexpectedEof,
            ErrorKind::OutOfMemory => Self::OutOfMemory,
            ErrorKind::Other => Self::Other,
            _ => Self::Uncategorized,
        }
    }
}
