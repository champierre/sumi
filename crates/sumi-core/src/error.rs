/// Errors returned by Sumi.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SumiError {
    /// The input could not be read as a PDF document.
    #[error("invalid PDF: {0}")]
    InvalidPdf(String),

    /// The input PDF is encrypted.
    #[error("encrypted PDFs are not supported")]
    EncryptedPdf,

    /// Strict mode is enabled and some parts of the document could not be converted.
    #[error("unsupported PDF features: {}", .0.join("; "))]
    Unsupported(Vec<String>),

    /// The conversion options are invalid.
    #[error("invalid options: {0}")]
    InvalidOptions(String),

    /// A resource limit (decompressed stream size, image size, nesting depth) was exceeded.
    #[error("resource limit exceeded: {0}")]
    LimitExceeded(String),

    /// Reading the input or writing the output failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A bug in Sumi or one of its dependencies.
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T, E = SumiError> = std::result::Result<T, E>;

/// Why a single object could not be converted.
///
/// `Unsupported` and `Invalid` leave the object unchanged and become report warnings;
/// `Limit` aborts the whole conversion.
#[derive(Debug)]
pub(crate) enum Problem {
    Unsupported(String),
    Invalid(String),
    Limit(String),
}

impl Problem {
    pub(crate) fn unsupported(msg: impl Into<String>) -> Self {
        Problem::Unsupported(msg.into())
    }

    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Problem::Invalid(msg.into())
    }
}

impl From<lopdf::Error> for Problem {
    fn from(err: lopdf::Error) -> Self {
        match err {
            lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { limit }) => {
                Problem::Limit(format!("a stream decompresses to more than {limit} bytes"))
            }
            lopdf::Error::Unimplemented(what) => Problem::Unsupported(what.to_string()),
            other => Problem::Invalid(other.to_string()),
        }
    }
}
