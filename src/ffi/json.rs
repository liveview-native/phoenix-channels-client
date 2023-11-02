use std::collections::HashMap;
use std::fmt;
use std::fmt::{Display, Formatter};

use crate::ffi::io::error::IoError;

/// Replicates [serde_json::value::Value], but with `uniffi` support.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum JSON {
    /// JSON `null`
    Null,
    /// JSON `true` or `false`
    Bool {
        /// The boolean
        bool: bool,
    },
    /// JSON integer or float
    Number {
        /// The integer or float
        number: Number,
    },
    /// JSON string
    String {
        /// The string
        string: String,
    },
    /// JSON array of JSON
    Array {
        /// The array
        array: Vec<JSON>,
    },
    /// JSON object
    Object {
        /// The object
        object: HashMap<String, JSON>,
    },
}
#[uniffi::export]
impl JSON {
    /// Deserializes JSON serialized to a string to `JSON` or errors with a
    /// [JSONDeserializationError] if the string is invalid JSON.
    #[uniffi::constructor]
    pub fn deserialize(serialized: String) -> Result<Self, JSONDeserializationError> {
        serde_json::from_str::<serde_json::Value>(serialized.as_str())
            .map(From::from)
            .map_err(From::from)
    }
}
impl Display for JSON {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let serde_json_value: serde_json::Value = self.clone().into();
        write!(f, "{}", serde_json_value)
    }
}
impl From<JSON> for serde_json::Value {
    fn from(value: JSON) -> Self {
        match value {
            JSON::Null => serde_json::Value::Null,
            JSON::Bool { bool } => serde_json::Value::Bool(bool),
            JSON::Number { number } => serde_json::Value::Number(number.into()),
            JSON::String { string } => serde_json::Value::String(string),
            JSON::Array { array } => {
                serde_json::Value::Array(array.into_iter().map(From::from).collect())
            }
            JSON::Object { object } => serde_json::to_value(
                object
                    .into_iter()
                    .map(|(key, value)| (key, serde_json::Value::from(value)))
                    .collect::<HashMap<String, serde_json::Value>>(),
            )
            .unwrap(),
        }
    }
}
impl From<serde_json::Value> for JSON {
    fn from(serde_value_ref: serde_json::Value) -> Self {
        match serde_value_ref {
            serde_json::Value::Null => JSON::Null,
            serde_json::Value::Bool(bool) => JSON::Bool { bool },
            serde_json::Value::Number(number) => JSON::Number {
                number: number.into(),
            },
            serde_json::Value::String(string) => JSON::String { string },
            serde_json::Value::Array(array) => JSON::Array {
                array: array.into_iter().map(From::from).collect(),
            },
            serde_json::Value::Object(map) => JSON::Object {
                object: map
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
            },
        }
    }
}
impl From<&serde_json::Value> for JSON {
    fn from(serde_value_ref: &serde_json::Value) -> Self {
        match serde_value_ref {
            serde_json::Value::Null => JSON::Null,
            serde_json::Value::Bool(bool) => JSON::Bool { bool: *bool },
            serde_json::Value::Number(number) => JSON::Number {
                number: number.into(),
            },
            serde_json::Value::String(string) => JSON::String {
                string: string.clone(),
            },
            serde_json::Value::Array(array) => JSON::Array {
                array: array.iter().map(From::from).collect(),
            },
            serde_json::Value::Object(map) => JSON::Object {
                object: map
                    .iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
            },
        }
    }
}

/// Replicates [serde_json::number::Number] and [serde_json::number::N], but with `uniffi` support.
#[derive(Copy, Clone, Debug, PartialEq, uniffi::Enum)]
pub enum Number {
    PosInt { pos: u64 },
    NegInt { neg: i64 },
    Float { float: f64 },
}
// Implementing Eq is fine since any float values are always finite.
impl Eq for Number {}
impl From<Number> for serde_json::Number {
    fn from(number: Number) -> Self {
        match number {
            Number::PosInt { pos } => pos.into(),
            Number::NegInt { neg } => neg.into(),
            Number::Float { float } => Self::from_f64(float).unwrap(),
        }
    }
}
impl From<serde_json::Number> for Number {
    fn from(serde_number: serde_json::Number) -> Self {
        (&serde_number).into()
    }
}
impl From<&serde_json::Number> for Number {
    fn from(serde_number_ref: &serde_json::Number) -> Self {
        if serde_number_ref.is_u64() {
            Self::PosInt {
                pos: serde_number_ref.as_u64().unwrap(),
            }
        } else if serde_number_ref.is_i64() {
            Self::NegInt {
                neg: serde_number_ref.as_i64().unwrap(),
            }
        } else {
            assert!(serde_number_ref.is_f64());

            Self::Float {
                float: serde_number_ref.as_f64().unwrap(),
            }
        }
    }
}

/// Error from [JSON::deserialize]
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum JSONDeserializationError {
    /// There was error reading from the underlying IO device after reading `column` of `line`.
    #[error("IO error on line {line} column {column}: {io_error}")]
    IO {
        /// The line that was being read before the `io_error` occurred.
        line: u64,
        /// The column on `line` that was being read before the `io_error` occurred.
        column: u64,
        /// The underlying [IoError] that occurred on `line` at `column`.
        io_error: IoError,
    },
    /// There was a JSON syntax error at `line` on `column`.
    #[error("syntax error on line {line} column {column}")]
    Syntax {
        /// The line where the syntax error occurred.
        line: u64,
        /// The column on `line` where the syntax error occurred.
        column: u64,
    },
    /// The data such as a string or number could not be converted from JSON at `line` on `column`.
    #[error("data error on line {line} column {column}")]
    Data {
        /// The line where the invalid string or number occurred.
        line: u64,
        /// The column on `line` where the invalid string or number occurred.
        column: u64,
    },
    /// The EOF was reached at `line` on `column` while still parsing a JSON structure.
    #[error("End-Of-File on line {line} column {column}")]
    EOF {
        /// The last line that was read before the premature End-Of-File.
        line: u64,
        /// The last column on `line` that was read before the premature End-Of-File.
        column: u64,
    },
}

impl From<serde_json::Error> for JSONDeserializationError {
    fn from(serde_error: serde_json::Error) -> Self {
        let line = serde_error.line() as u64;
        let column = serde_error.column() as u64;

        match serde_error.classify() {
            serde_json::error::Category::Io => Self::IO {
                line,
                column,
                io_error: serde_error.io_error_kind().unwrap().into(),
            },
            serde_json::error::Category::Syntax => Self::Syntax { line, column },
            serde_json::error::Category::Data => Self::Data { line, column },
            serde_json::error::Category::Eof => Self::EOF { line, column },
        }
    }
}
