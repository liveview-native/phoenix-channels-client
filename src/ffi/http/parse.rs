/// [httparse::Error], but with `uniffi` support
/// An error in parsing.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error, uniffi::Error)]
pub enum HTTParseError {
    /// Invalid byte in header name.
    #[error("invalid header name")]
    HeaderName,
    /// Invalid byte in header value.
    #[error("invalid header value")]
    HeaderValue,
    /// Invalid byte in new line.
    #[error("invalid new line")]
    NewLine,
    /// Invalid byte in Response status.
    #[error("invalid response status")]
    Status,
    /// Invalid byte where token is required.
    #[error("invalid token")]
    Token,
    /// Parsed more headers than provided buffer can contain.
    #[error("too many headers")]
    TooManyHeaders,
    /// Invalid byte in HTTP version.
    #[error("invalid HTTP version")]
    Version,
}
impl From<&httparse::Error> for HTTParseError {
    fn from(rust_error: &httparse::Error) -> Self {
        match rust_error {
            httparse::Error::HeaderName => Self::HeaderName,
            httparse::Error::HeaderValue => Self::HeaderValue,
            httparse::Error::NewLine => Self::NewLine,
            httparse::Error::Status => Self::Status,
            httparse::Error::Token => Self::Token,
            httparse::Error::TooManyHeaders => Self::TooManyHeaders,
            httparse::Error::Version => Self::Version,
        }
    }
}
