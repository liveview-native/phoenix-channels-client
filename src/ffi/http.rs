pub(crate) mod parse;

use std::collections::HashMap;

use tokio_tungstenite::tungstenite::http::header::{InvalidHeaderName, InvalidHeaderValue};
use tokio_tungstenite::tungstenite::http::uri::{
    InvalidUri as TungsteniteInvalidUri, InvalidUriParts as TungsteniteInvalidUriParts,
};
use tokio_tungstenite::tungstenite::http::{
    method::InvalidMethod, status::InvalidStatusCode, Error as TungsteniteError,
    Response as TungsteniteResponse,
};

/// [http::error::Error], but with `uniffi` support
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum HttpError {
    #[error("invalid status code")]
    StatusCode,
    #[error("invalid method")]
    Method,
    #[error("invalid URI: {invalid_uri}")]
    Uri { invalid_uri: String },
    #[error("invalid URI parts: {invalid_uri}")]
    UriParts { invalid_uri: String },
    #[error("invalid header name")]
    HeaderName,
    #[error("invalid header value")]
    HeaderValue,
}
impl From<TungsteniteError> for HttpError {
    fn from(rust_error: TungsteniteError) -> Self {
        (&rust_error).into()
    }
}
impl From<&TungsteniteError> for HttpError {
    fn from(rust_error: &TungsteniteError) -> Self {
        let rust_error_ref = rust_error.get_ref();

        if let Some(_) = rust_error_ref.downcast_ref::<InvalidStatusCode>() {
            Self::StatusCode
        } else if let Some(_) = rust_error_ref.downcast_ref::<InvalidMethod>() {
            Self::Method
        } else if let Some(invalid_uri) = rust_error_ref.downcast_ref::<TungsteniteInvalidUri>() {
            Self::Uri {
                invalid_uri: invalid_uri.to_string(),
            }
        } else if let Some(invalid_uri_parts) =
            rust_error_ref.downcast_ref::<TungsteniteInvalidUriParts>()
        {
            Self::UriParts {
                invalid_uri: invalid_uri_parts.to_string(),
            }
        } else if let Some(_) = rust_error_ref.downcast_ref::<InvalidHeaderName>() {
            Self::HeaderName
        } else if let Some(_) = rust_error_ref.downcast_ref::<InvalidHeaderValue>() {
            Self::HeaderValue
        } else {
            panic!("Unexpected http::Error: {:?}", rust_error)
        }
    }
}

#[derive(Copy, Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum InvalidUri {
    #[error("invalid uri character")]
    InvalidUriChar,
    #[error("invalid scheme")]
    InvalidScheme,
    #[error("invalid authority")]
    InvalidAuthority,
    #[error("invalid port")]
    InvalidPort,
    #[error("invalid format")]
    InvalidFormat,
    #[error("scheme missing")]
    SchemeMissing,
    #[error("authority missing")]
    AuthorityMissing,
    #[error("path missing")]
    PathAndQueryMissing,
    #[error("uri too long")]
    TooLong,
    #[error("empty string")]
    Empty,
    #[error("scheme too long")]
    SchemeTooLong,
}

/// [http::response::Response], but without the generics that `uniffi` does not support and with `uniffi` support
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Response {
    pub status_code: u16,
    pub headers: HashMap<String, Vec<String>>,
    pub body: Option<Vec<u8>>,
}
impl From<TungsteniteResponse<Option<Vec<u8>>>> for Response {
    fn from(rust_response: TungsteniteResponse<Option<Vec<u8>>>) -> Self {
        (&rust_response).into()
    }
}
impl From<&TungsteniteResponse<Option<Vec<u8>>>> for Response {
    fn from(rust_response: &TungsteniteResponse<Option<Vec<u8>>>) -> Self {
        let mut headers: HashMap<String, Vec<String>> = HashMap::new();

        for (header_name, header_value) in rust_response.headers().iter() {
            let values = headers.entry(header_name.to_string()).or_default();

            // TODO should we propagate bad headers?
            if let Ok(header_value) = header_value.to_str() {
                values.push(header_value.to_string())
            }
        }

        Self {
            status_code: rust_response.status().as_u16(),
            headers,
            body: rust_response.body().clone(),
        }
    }
}
