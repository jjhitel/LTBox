use std::cmp::min;
use std::io::{self, BufRead, Read, Write};

use qdl::types::{FirehoseConfiguration, QdlBackend, QdlChan};

const ACK: &[u8] = b"<data><response value=\"ACK\"/></data>";
const PAYLOAD: &[u8] = b"abcdefgh";

struct MockChannel {
    incoming: Vec<u8>,
    cursor: usize,
    writes: Vec<u8>,
    config: FirehoseConfiguration,
    read_calls: usize,
}

impl MockChannel {
    fn new(payload: &[u8]) -> Self {
        let mut incoming = ACK.to_vec();
        incoming.extend_from_slice(payload);
        incoming.extend_from_slice(ACK);

        Self {
            incoming,
            cursor: 0,
            writes: Vec::new(),
            config: FirehoseConfiguration {
                recv_buffer_size: 4,
                storage_sector_size: 4,
                backend: QdlBackend::Serial,
                ..FirehoseConfiguration::default()
            },
            read_calls: 0,
        }
    }
}

impl Read for MockChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_calls += 1;
        let available = self.incoming.len() - self.cursor;
        let count = min(buf.len(), available);
        buf[..count].copy_from_slice(&self.incoming[self.cursor..self.cursor + count]);
        self.cursor += count;
        Ok(count)
    }
}

impl BufRead for MockChannel {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Ok(&self.incoming[self.cursor..])
    }

    fn consume(&mut self, amount: usize) {
        self.cursor = min(self.cursor + amount, self.incoming.len());
    }
}

impl Write for MockChannel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl QdlChan for MockChannel {
    fn fh_config(&self) -> &FirehoseConfiguration {
        &self.config
    }

    fn mut_fh_config(&mut self) -> &mut FirehoseConfiguration {
        &mut self.config
    }
}

struct PartialWriter {
    max_per_write: usize,
    bytes: Vec<u8>,
}

impl Write for PartialWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let count = min(self.max_per_write, buf.len());
        self.bytes.extend_from_slice(&buf[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct ZeroWriter;

impl Write for ZeroWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PartialThenErrorWriter {
    bytes: Vec<u8>,
    wrote_partial: bool,
}

impl Write for PartialThenErrorWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.wrote_partial {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "sink failed after partial write",
            ));
        }

        self.wrote_partial = true;
        let count = min(2, buf.len());
        self.bytes.extend_from_slice(&buf[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct InterruptedOnceWriter {
    interrupted: bool,
    bytes: Vec<u8>,
}

impl Write for InterruptedOnceWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "temporary sink interruption",
            ));
        }

        let count = min(2, buf.len());
        self.bytes.extend_from_slice(&buf[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn firehose_read_storage_preserves_payload_with_repeated_partial_writes() {
    let mut channel = MockChannel::new(PAYLOAD);
    let mut output = PartialWriter {
        max_per_write: 1,
        bytes: Vec::new(),
    };

    qdl::firehose_read_storage(&mut channel, &mut output, 2, 0, 0, 0)
        .expect("storage read should succeed");

    assert_eq!(output.bytes, PAYLOAD);
    assert_eq!(channel.read_calls, 2, "both data chunks should be consumed");
    assert_eq!(channel.cursor, channel.incoming.len(), "final ACK consumed");
    assert!(String::from_utf8_lossy(&channel.writes).contains("<read"));
}

#[test]
fn firehose_read_storage_reports_write_zero() {
    let mut channel = MockChannel::new(PAYLOAD);
    let mut output = ZeroWriter;

    let error = qdl::firehose_read_storage(&mut channel, &mut output, 2, 0, 0, 0)
        .expect_err("a zero-length write must fail");

    assert_eq!(
        error.downcast_ref::<io::Error>().map(io::Error::kind),
        Some(io::ErrorKind::WriteZero)
    );
    assert_eq!(
        channel.read_calls, 1,
        "the second data chunk must remain unread"
    );
}

#[test]
fn firehose_read_storage_propagates_partial_write_error_without_reading_next_chunk() {
    let mut channel = MockChannel::new(PAYLOAD);
    let mut output = PartialThenErrorWriter {
        bytes: Vec::new(),
        wrote_partial: false,
    };

    let error = qdl::firehose_read_storage(&mut channel, &mut output, 2, 0, 0, 0)
        .expect_err("the sink error must be returned");

    assert_eq!(output.bytes, b"ab");
    assert_eq!(
        channel.read_calls, 1,
        "the second data chunk must remain unread"
    );
    let sink_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<io::Error>());
    assert_eq!(
        sink_error.map(io::Error::kind),
        Some(io::ErrorKind::BrokenPipe)
    );
    assert_eq!(
        sink_error.map(ToString::to_string).as_deref(),
        Some("sink failed after partial write")
    );
}

#[test]
fn firehose_read_storage_retries_interrupted_writes() {
    let mut channel = MockChannel::new(PAYLOAD);
    let mut output = InterruptedOnceWriter {
        interrupted: false,
        bytes: Vec::new(),
    };

    qdl::firehose_read_storage(&mut channel, &mut output, 2, 0, 0, 0)
        .expect("Interrupted should be retried by write_all");

    assert_eq!(output.bytes, PAYLOAD);
    assert_eq!(channel.read_calls, 2);
}
