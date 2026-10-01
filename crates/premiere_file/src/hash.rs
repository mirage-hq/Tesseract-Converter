use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

/// Hash only bytes accepted by the underlying writer, without rereading output.
/// Consume after the outer encoder finishes, so its footer is included as well.
#[derive(Debug)]
pub(crate) struct DigestWriter<W> {
    inner: W,
    digest: Sha256,
}

impl<W> DigestWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
        }
    }

    pub(crate) fn into_parts(self) -> (W, [u8; 32]) {
        (self.inner, self.digest.finalize().into())
    }
}

impl<W: Write> Write for DigestWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.digest.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub(crate) fn hash(path: &Path) -> std::io::Result<String> {
    hash_reader(File::open(path)?)
}

pub(crate) fn hash_reader(reader: impl Read) -> std::io::Result<String> {
    Ok(format!("{:x}", digest_reader(reader)?))
}

pub(crate) fn digest_reader(mut reader: impl Read) -> io::Result<sha2::digest::Output<Sha256>> {
    let mut sha = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    Ok(sha.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ShortWriter {
        bytes: Vec<u8>,
        limit: usize,
    }

    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let count = bytes.len().min(2).min(self.limit - self.bytes.len());
            if count == 0 && !bytes.is_empty() {
                return Err(io::Error::other("injected write failure"));
            }
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("injected flush failure"))
        }
    }

    #[test]
    fn digest_tracks_only_accepted_bytes_across_short_writes_and_errors() {
        let bytes = b"partial writes must not hash rejected bytes";
        for limit in [bytes.len(), 5] {
            let mut writer = DigestWriter::new(ShortWriter {
                bytes: Vec::new(),
                limit,
            });
            assert_eq!(writer.write_all(bytes).is_ok(), limit == bytes.len());
            assert!(writer.flush().is_err());
            let (output, digest) = writer.into_parts();
            assert_eq!(output.bytes, bytes[..limit]);
            let expected: [u8; 32] = Sha256::digest(&bytes[..limit]).into();
            assert_eq!(digest, expected);
        }
    }
}
