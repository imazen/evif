use core::fmt;

use crate::FourCc;

/// Why a file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The file violates the format.
    Corrupt(&'static str),
    /// A critical chunk or rendition format this reader does not know.
    Unsupported(FourCc),
    /// A major version this reader does not know.
    UnsupportedVersion(u8),
    /// A JSON chunk failed to parse or serialize.
    Json,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(why) => write!(f, "corrupt EVIF: {why}"),
            Self::Unsupported(tag) => {
                write!(
                    f,
                    "unsupported critical tag {:?}",
                    tag.escape_ascii().to_string()
                )
            }
            Self::UnsupportedVersion(v) => write!(f, "unsupported EVIF major version {v}"),
            Self::Json => f.write_str("malformed JSON chunk"),
        }
    }
}

impl core::error::Error for Error {}

use alloc::string::ToString;
