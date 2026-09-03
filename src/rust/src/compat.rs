use std::io;

use crate::BootstrapError;

impl From<BootstrapError> for io::Error {
    fn from(error: BootstrapError) -> Self {
        io::Error::new(io::ErrorKind::InvalidInput, error)
    }
}
