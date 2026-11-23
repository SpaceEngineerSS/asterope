use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsteropeError {
    CatalogTruncated,
    CatalogBadMagic,
    CatalogUnsupportedVersion,
    CatalogChecksumMismatch,
    InvalidCamera,
    InvalidFrame,
    InvalidParameter,
    TooFewObservations,
    DegenerateGeometry,
    IndexTooLarge,
    IndexTruncated,
    IndexBadMagic,
    IndexUnsupportedVersion,
    IndexChecksumMismatch,
    IndexCorrupt,
    IndexCatalogMismatch,
}

impl fmt::Display for AsteropeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            AsteropeError::CatalogTruncated => "catalog data is truncated",
            AsteropeError::CatalogBadMagic => "catalog magic bytes do not match",
            AsteropeError::CatalogUnsupportedVersion => "catalog version is not supported",
            AsteropeError::CatalogChecksumMismatch => "catalog checksum mismatch",
            AsteropeError::InvalidCamera => "camera parameters are invalid",
            AsteropeError::InvalidFrame => "frame dimensions do not match the pixel buffer",
            AsteropeError::InvalidParameter => "parameter is out of range",
            AsteropeError::TooFewObservations => "not enough vector observations",
            AsteropeError::DegenerateGeometry => "observation geometry is degenerate",
            AsteropeError::IndexTooLarge => "pair index exceeds the configured limit",
            AsteropeError::IndexTruncated => "index data is truncated",
            AsteropeError::IndexBadMagic => "index magic bytes do not match",
            AsteropeError::IndexUnsupportedVersion => "index version is not supported",
            AsteropeError::IndexChecksumMismatch => "index checksum mismatch",
            AsteropeError::IndexCorrupt => "index header is inconsistent",
            AsteropeError::IndexCatalogMismatch => "index was not built from this catalog",
        };
        f.write_str(text)
    }
}
