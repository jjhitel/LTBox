// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
use anstream::println;
use anyhow::Context;
use anyhow::Result;
use indexmap::{Equivalent, IndexMap};
use owo_colors::OwoColorize;
use parsers::firehose_parser_ack_nak;
use serde::{Deserialize, Serialize};
use std::cmp::min;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::str::{self, FromStr};
use types::FirehoseResetMode;
use types::FirehoseStatus;
use types::FirehoseStorageType;
use types::QdlBackend;
use types::QdlChan;
use types::QdlReadWrite;

use anyhow::bail;
use pbr::{ProgressBar, Units};
use xmltree::{self, Element, XMLNode};

pub mod parsers;
pub mod sahara;
#[cfg(feature = "serial")]
pub mod serial;
pub mod types;
#[cfg(feature = "usb")]
pub mod usb;

pub const SAHARA_ID_EHOSTDL_IMG: usize = 13;

const CPIO_MAGIC: &[u8; 6] = b"070701";

#[repr(C)]
#[derive(Debug, Serialize, Deserialize)]
struct CpioNewcHeader {
    c_magic: [u8; 6],
    c_ino: [u8; 8],
    c_mode: [u8; 8],
    c_uid: [u8; 8],
    c_gid: [u8; 8],
    c_nlink: [u8; 8],
    c_mtime: [u8; 8],
    c_filesize: [u8; 8],
    c_devmajor: [u8; 8],
    c_devminor: [u8; 8],
    c_rdevmajor: [u8; 8],
    c_rdevminor: [u8; 8],
    c_namesize: [u8; 8],
    c_check: [u8; 8],
}

fn align_up_4(n: usize) -> usize {
    (n + 3) & !3
}

fn parse_ascii_hex_u32(field: &[u8]) -> Result<u32> {
    let s = std::str::from_utf8(field)?;
    if s.len() != 8 {
        bail!("invalid cpio header field size");
    }

    u32::from_str_radix(s, 16).map_err(|_| anyhow::anyhow!("invalid hex field \"{}\"", s))
}

fn decode_programmer_archive(blob: &[u8], images: &mut Vec<Option<Vec<u8>>>) -> Result<bool> {
    if blob.len() < size_of::<CpioNewcHeader>() || &blob[..6] != CPIO_MAGIC {
        return Ok(false);
    }

    let mut ptr = 0usize;

    loop {
        if ptr + size_of::<CpioNewcHeader>() > blob.len() {
            bail!("programmer archive is truncated");
        }

        let hdr =
            bincode::deserialize::<CpioNewcHeader>(&blob[ptr..ptr + size_of::<CpioNewcHeader>()])?;
        if !hdr.c_magic.equivalent(CPIO_MAGIC) {
            bail!("expected cpio header in programmer archive");
        }

        let filesize = parse_ascii_hex_u32(&hdr.c_filesize)? as usize;
        let namesize = parse_ascii_hex_u32(&hdr.c_namesize)? as usize;

        ptr += size_of::<CpioNewcHeader>();
        if ptr + namesize > blob.len() {
            bail!("programmer archive is truncated");
        }

        if namesize == 0 {
            bail!("missing filename in programmer archive entry");
        }

        let name_raw = &blob[ptr..ptr + namesize];
        let name_end = name_raw
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(name_raw.len());
        let name = std::str::from_utf8(&name_raw[..name_end])
            .map_err(|_| anyhow::anyhow!("invalid utf8 in programmer archive filename"))?;

        if name == "TRAILER!!!" {
            break;
        }

        let id_str = name.split(':').next().unwrap_or("");
        if id_str.is_empty() {
            bail!("missing image id in programmer archive entry");
        }

        let id = id_str
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("invalid decimal image id \"{}\"", id_str))?;
        if id == 0 {
            bail!("invalid image id \"{}\" in programmer archive", id_str);
        }

        ptr += namesize;
        ptr = align_up_4(ptr);
        if ptr + filesize > blob.len() {
            bail!("programmer archive is truncated");
        }

        let file_data = &blob[ptr..ptr + filesize];
        if id as usize >= images.len() {
            images.resize(id as usize + 1, None);
        }
        images[id as usize] = Some(file_data.to_vec());

        ptr += filesize;
        ptr = align_up_4(ptr);
    }

    Ok(true)
}

/// Load Sahara programmer image(s) from disk.
///
/// If `path` points to a CPIO `newc` archive, this decodes entries named like
/// `<id>:<name>` (or just `<id>`) and stores each file in its Sahara image slot.
/// Otherwise, the file is returned as a single-slot image list, preserving
/// legacy single-image Sahara behavior.
pub fn load_programmer_images(path: impl AsRef<Path>) -> Result<Vec<Option<Vec<u8>>>> {
    let blob = fs::read(path.as_ref())
        .with_context(|| format!("Couldn't read programmer image {}", path.as_ref().display()))?;
    let mut images: Vec<Option<Vec<u8>>> = Vec::new();

    if decode_programmer_archive(&blob, &mut images)? {
        return Ok(images);
    }

    Ok(vec![Some(blob)])
}

pub fn setup_target_device(
    backend: QdlBackend,
    _serial_no: Option<String>,
    _port: Option<String>,
) -> Result<Box<dyn QdlReadWrite>> {
    match backend {
        #[cfg(feature = "serial")]
        QdlBackend::Serial => match serial::setup_serial_device(_port) {
            Ok(d) => Ok(Box::new(d)),
            Err(e) => Err(e),
        },
        #[cfg(feature = "usb")]
        QdlBackend::Usb => match usb::setup_usb_device(_serial_no) {
            Ok(d) => Ok(Box::new(d)),
            Err(e) => Err(e),
        },
        // If all back-ends are compiled in, this throws a warning
        #[allow(unreachable_patterns)]
        _ => bail!("The {:?} backend is not supported in this build", backend),
    }
}

/// Wrapper for easily creating Firehose-y XML packets
fn firehose_xml_setup(op: &str, kvps: &[(&str, &str)]) -> anyhow::Result<Vec<u8>> {
    let mut xml = Element::new("data");
    let mut op_node = Element::new(op);
    for kvp in kvps.iter() {
        op_node
            .attributes
            .insert(kvp.0.to_owned(), kvp.1.to_owned());
    }

    xml.children.push(XMLNode::Element(op_node));

    // TODO: define a more verbose level
    // println!("SEND: {}", format!("{:?}", xml).bright_cyan());

    let mut buf = Vec::<u8>::new();
    xml.write(&mut buf)?;

    Ok(buf)
}

/// Main Firehose XML reading function
pub fn firehose_read<T: QdlChan>(
    channel: &mut T,
    response_parser: fn(&mut T, &IndexMap<String, String>) -> Result<FirehoseStatus, anyhow::Error>,
) -> Result<FirehoseStatus, anyhow::Error> {
    let mut got_any_data = false;
    let mut pending: Vec<u8> = Vec::new();

    loop {
        // Use BufRead to peek at available data
        let available = match channel.fill_buf() {
            Ok(buf) => buf,
            Err(e) => match e.kind() {
                // In some cases (like with welcome messages), there's no acking
                // and a timeout is the "end of data" marker instead..
                std::io::ErrorKind::TimedOut if got_any_data => return Ok(FirehoseStatus::Ack),
                std::io::ErrorKind::TimedOut => return Err(e.into()),
                _ => return Err(e.into()),
            },
        };

        got_any_data = true;

        // When channel is a non-packetized BufRead (e.g. serial) XML documents
        // are not separated from each other, or from rawmode data. Search for
        // </data> in the BufRead stream to find the end of the current
        // message.
        let data_end_marker = b"</data>";

        let pending_length = pending.len();
        pending.extend_from_slice(available);

        // Search for the end marker in the pending data
        let end_pos = pending
            .windows(data_end_marker.len())
            .position(|window| window == data_end_marker);

        if let Some(pos) = end_pos {
            let xml_end = pos + data_end_marker.len();

            // xml_end is relative "pending", we need to consume only new the tail
            channel.consume(xml_end - pending_length);

            // Only parse the XML portion
            let xml_chunk = &pending[..xml_end];
            let xml = match xmltree::Element::parse(xml_chunk) {
                Ok(x) => x,
                Err(e) => {
                    // Consume the bad data and continue
                    bail!("Failed to parse XML: {}", e);
                }
            };

            // The current message might have started in "pending", so clear it
            // now. No need to do this if we're bailing above, as it's a local
            // resource.
            pending.clear();

            if xml.name != "data" {
                // TODO: define a more verbose level
                if channel.fh_config().verbose_firehose {
                    println!("{:?}", xml);
                }
                bail!("Got a firehose packet without a data tag");
            }

            // The spec expects there's always a single node only
            if let Some(XMLNode::Element(e)) = xml.children.first() {
                // Check for a 'log' node and print out the message
                if e.name == "log" {
                    // The last message within the initial logspam should be this
                    // Try to match on it to not pay the USB xfer timeout penalty each time
                    if let Some(val) = e.attributes.get_key_value("value")
                        && val.1.starts_with("INFO: End of supported functions")
                    {
                        return Ok(FirehoseStatus::Ack);
                    }
                    if channel.fh_config().skip_firehose_log {
                        continue;
                    }

                    println!(
                        "LOG: {}",
                        e.attributes
                            .get("value")
                            .to_owned()
                            .unwrap_or(&String::from("<garbage log data>"))
                            .bright_black()
                    );

                    continue;
                }

                // DEBUG: "print out incoming packets"
                // TODO: define a more verbose level
                if channel.fh_config().verbose_firehose {
                    println!("RECV: {}", format!("{e:?}").magenta());
                }

                // TODO: Use std::intrinsics::unlikely after it exits nightly
                if e.attributes.get("AttemptRetry").is_some() {
                    // Restart the outer loop instead of recursing to avoid stack overflow
                    got_any_data = false;
                    continue;
                } else if e.attributes.get("AttemptRestart").is_some() {
                    // TODO: handle this automagically
                    firehose_reset(channel, &FirehoseResetMode::ResetToEdl, 0)?;
                    bail!("Firehose requested a restart. Run the program again.");
                }

                // Pass other nodes to specialized parsers
                return response_parser(channel, &e.attributes);
            }
        } else {
            // Didn't find the tail of the XML document in "pending" +
            // "available", consume the data into "pending" to let fill_buf()
            // read more data from the underlying Read.
            let available_len = available.len();
            channel.consume(available_len);
        }
    }
}

/// Send a Firehose packet
pub fn firehose_write<T: QdlChan>(channel: &mut T, buf: &mut [u8]) -> anyhow::Result<()> {
    let mut b = buf.to_vec();

    // XML can't be n * 512 bytes long by fh spec
    if !buf.is_empty() && buf.len().is_multiple_of(512) {
        println!("{}", "INFO: Appending '\n' to outgoing XML".bright_black());
        b.push(b'\n');
    }

    match channel.write_all(&b) {
        Ok(_) => Ok(()),
        // Assume FH will hang after NAK..
        Err(_) => firehose_reset(channel, &FirehoseResetMode::ResetToEdl, 0),
    }
}

/// Send a Firehose packet and check for ack/nak
pub fn firehose_write_getack<T: QdlChan>(
    channel: &mut T,
    buf: &mut [u8],
    couldnt_what: String,
) -> anyhow::Result<()> {
    firehose_write(channel, buf)?;

    match firehose_read::<T>(channel, firehose_parser_ack_nak) {
        Ok(FirehoseStatus::Ack) => Ok(()),
        Ok(FirehoseStatus::Nak) => {
            // Assume FH will hang after NAK..
            firehose_reset(channel, &FirehoseResetMode::ResetToEdl, 0)?;
            Err(anyhow::Error::msg(format!("Couldn't {couldnt_what}")))
        }
        Err(e) => Err(e),
    }
}

/// Test performance without sample data
pub fn firehose_benchmark<T: QdlChan>(
    channel: &mut T,
    trials: u32,
    test_write_perf: bool,
) -> anyhow::Result<()> {
    let mut xml = firehose_xml_setup(
        "benchmark",
        &[
            ("trials", &trials.to_string()),
            (
                "TestWritePerformance",
                &(test_write_perf as u32).to_string(),
            ),
            (
                "TestReadPerformance",
                &(!test_write_perf as u32).to_string(),
            ),
        ],
    )?;

    firehose_write_getack(channel, &mut xml, "issue a NOP".to_owned())
}

/// Send a "Hello"-type packet to the Device
pub fn firehose_configure<T: QdlChan>(
    channel: &mut T,
    skip_storage_init: bool,
) -> anyhow::Result<()> {
    let config = channel.fh_config();
    // Spec requirement
    assert!(
        config
            .send_buffer_size
            .is_multiple_of(config.storage_sector_size)
    );
    // Sanity requirement
    assert!(
        config
            .recv_buffer_size
            .is_multiple_of(config.storage_sector_size)
    );
    let mut xml = firehose_xml_setup(
        "configure",
        &[
            ("AckRawDataEveryNumPackets", "0"), // TODO: (low prio)
            (
                "SkipWrite",
                &(channel.fh_config().bypass_storage as u32).to_string(),
            ),
            ("SkipStorageInit", &(skip_storage_init as u32).to_string()),
            ("MemoryName", &config.storage_type.to_string()),
            ("AlwaysValidate", &(config.hash_packets as u32).to_string()),
            ("Verbose", &(config.verbose_firehose as u32).to_string()),
            ("MaxDigestTableSizeInBytes", "8192"), // TODO: (low prio)
            (
                "MaxPayloadSizeToTargetInBytes",
                &config.send_buffer_size.to_string(),
            ),
            // Zero-length-packet aware host
            ("ZLPAwareHost", "1"),
        ],
    )?;

    firehose_write(channel, &mut xml)
}

/// Do nothing, hopefully succesfully
pub fn firehose_nop<T: QdlChan>(channel: &mut T) -> anyhow::Result<()> {
    let mut xml = firehose_xml_setup("nop", &[("value", "ping")])?;

    firehose_write_getack(channel, &mut xml, "issue a NOP".to_owned())
}

/// Get information about the physical partition of a storage medium (e.g. LUN)
/// Prints to \<log\> only
pub fn firehose_get_storage_info<T: QdlChan>(
    channel: &mut T,
    phys_part_idx: u8,
) -> anyhow::Result<()> {
    let mut xml = firehose_xml_setup(
        "getstorageinfo",
        &[("physical_partition_number", &phys_part_idx.to_string())],
    )?;

    firehose_write(channel, &mut xml)?;

    firehose_read::<T>(channel, firehose_parser_ack_nak).and(Ok(()))
}

/// Alter Device (TODO: or Host) storage
pub fn firehose_patch<T: QdlChan>(
    channel: &mut T,
    byte_off: u64,
    slot: u8,
    phys_part_idx: u8,
    size: u64,
    start_sector: &str,
    val: &str,
) -> anyhow::Result<()> {
    let mut xml: Vec<u8> = firehose_xml_setup(
        "patch",
        &[
            (
                "SECTOR_SIZE_IN_BYTES",
                &channel.fh_config().storage_sector_size.to_string(),
            ),
            ("byte_offset", &byte_off.to_string()),
            ("filename", "DISK"), // DISK means "patch device's storage"
            ("slot", &slot.to_string()),
            ("physical_partition_number", &phys_part_idx.to_string()),
            ("size_in_bytes", &size.to_string()),
            ("start_sector", start_sector),
            ("value", val),
        ],
    )?;

    firehose_write_getack(channel, &mut xml, "patch".to_string())
}

/// Peek at memory
/// Prints to \<log\> only
pub fn firehose_peek<T: QdlChan>(
    channel: &mut T,
    addr: u64,
    byte_count: u64,
) -> anyhow::Result<()> {
    if channel.fh_config().skip_firehose_log {
        println!(
            "{}",
            "Warning: firehose <peek> only prints to <log>, remove --skip-firehose-log"
                .bright_red()
        );
    }

    let mut xml: Vec<u8> = firehose_xml_setup(
        "peek",
        &[
            ("address64", &addr.to_string()),
            ("size_in_bytes", &byte_count.to_string()),
        ],
    )?;

    firehose_write_getack(channel, &mut xml, format!("peek @ {addr:#x}"))
}

/// Poke at memory
/// This can lead to lock-ups and resets
// TODO:x
pub fn firehose_poke<T: QdlChan>(
    channel: &mut T,
    addr: u64,
    // TODO: byte count is 1..=8
    byte_count: u8,
    val: u64,
) -> anyhow::Result<()> {
    let mut xml: Vec<u8> = firehose_xml_setup(
        "poke",
        &[
            ("address64", &addr.to_string()),
            ("size_in_bytes", &byte_count.to_string()),
            ("value", &val.to_string()),
        ],
    )?;

    firehose_write_getack(channel, &mut xml, format!("poke @ {addr:#x}"))
}

/// Write to Device storage
pub fn firehose_program_storage<T: QdlChan>(
    channel: &mut T,
    data: &mut impl Read,
    label: &str,
    num_sectors: usize,
    slot: u8,
    phys_part_idx: u8,
    start_sector: &str,
) -> anyhow::Result<()> {
    firehose_program_storage_with_progress(
        channel,
        data,
        label,
        num_sectors,
        slot,
        phys_part_idx,
        start_sector,
        |_, _| {},
    )
}

/// Write to Device storage, reporting byte progress to `on_progress`.
///
/// `on_progress` receives `(completed_bytes, total_bytes)`. It is invoked with
/// `0` after the device accepts the `<program>` command, then again after each
/// successful chunk write. Terminal `pbr` output is unchanged.
pub fn firehose_program_storage_with_progress<T, F>(
    channel: &mut T,
    data: &mut impl Read,
    label: &str,
    num_sectors: usize,
    slot: u8,
    phys_part_idx: u8,
    start_sector: &str,
    mut on_progress: F,
) -> anyhow::Result<()>
where
    T: QdlChan,
    F: FnMut(u64, u64),
{
    let mut sectors_left = num_sectors;
    let mut xml = firehose_xml_setup(
        "program",
        &[
            (
                "SECTOR_SIZE_IN_BYTES",
                &channel.fh_config().storage_sector_size.to_string(),
            ),
            ("num_partition_sectors", &num_sectors.to_string()),
            ("slot", &slot.to_string()),
            ("physical_partition_number", &phys_part_idx.to_string()),
            ("start_sector", start_sector),
            (
                "read_back_verify",
                &(channel.fh_config().read_back_verify as u32).to_string(),
            ),
        ],
    )?;

    firehose_write(channel, &mut xml)?;

    if firehose_read::<T>(channel, firehose_parser_ack_nak)? != FirehoseStatus::Ack {
        bail!("<program> was NAKed. Did you set sector-size correctly?");
    }

    let total_bytes = (sectors_left * channel.fh_config().storage_sector_size) as u64;
    let mut pb = ProgressBar::new(total_bytes);
    pb.show_time_left = true;
    pb.message(&format!("Sending partition {label}: "));
    pb.set_units(Units::Bytes);

    // Structured progress for GUI consumers; terminal bar stays independent.
    on_progress(0, total_bytes);
    let mut completed_bytes: u64 = 0;

    while sectors_left > 0 {
        let chunk_size_sectors = min(
            sectors_left,
            channel.fh_config().send_buffer_size / channel.fh_config().storage_sector_size,
        );
        let mut buf = vec![
            0u8;
            min(
                channel.fh_config().send_buffer_size,
                chunk_size_sectors * channel.fh_config().storage_sector_size,
            )
        ];
        let _ = data.read(&mut buf).unwrap();

        let n = channel
            .write(&buf)
            .with_context(|| format!("Error sending data for partition {label}"))?;
        if n != chunk_size_sectors * channel.fh_config().storage_sector_size {
            bail!("Wrote an unexpected number of bytes ({})", n);
        }

        sectors_left -= chunk_size_sectors;
        let chunk_bytes = (chunk_size_sectors * channel.fh_config().storage_sector_size) as u64;
        completed_bytes = completed_bytes.saturating_add(chunk_bytes);
        pb.add(chunk_bytes);
        on_progress(completed_bytes, total_bytes);
    }
    // Close the bar with a newline. `pbr` only ever rewrites its single row
    // with `\r`, so without this the final 100% state stays on the current
    // line and the next log message is appended to it.
    pb.finish_println("");

    // The USB `Write` impl already terminates every transfer through
    // `EndpointWrite::submit_end()` — a ZLP when the payload is a multiple
    // of the bulk max-packet size, a short packet otherwise. Emitting an
    // extra explicit `write(&[])` here put a second, stray zero-length OUT
    // transfer on the wire after a packet-aligned partition; Firehose
    // byte-counts the partition and stops reading OUT once it has all the
    // sectors, so that stray ZLP stalls the next `<program>` write
    // indefinitely. Rely on `submit_end()` for the single terminator.

    if firehose_read::<T>(channel, firehose_parser_ack_nak)? != FirehoseStatus::Ack {
        bail!("Failed to complete 'write' op");
    }

    Ok(())
}

/// Get a SHA256 digest of a portion of Device storage
pub fn firehose_checksum_storage<T: QdlChan>(
    channel: &mut T,
    num_sectors: usize,
    phys_part_idx: u8,
    start_sector: u64,
) -> anyhow::Result<()> {
    let mut xml = firehose_xml_setup(
        "getsha256digest",
        &[
            (
                "SECTOR_SIZE_IN_BYTES",
                &channel.fh_config().storage_sector_size.to_string(),
            ),
            ("num_partition_sectors", &num_sectors.to_string()),
            ("physical_partition_number", &phys_part_idx.to_string()),
            ("start_sector", &start_sector.to_string()),
        ],
    )?;

    firehose_write(channel, &mut xml)?;

    // TODO: figure out some sane way to figure out the timeout
    if firehose_read::<T>(channel, firehose_parser_ack_nak)? != FirehoseStatus::Ack {
        bail!("Checksum request was NAKed");
    }

    Ok(())
}

/// Read (sector-aligned) parts of storage.
pub fn firehose_read_storage(
    channel: &mut impl QdlChan,
    out: &mut impl Write,
    num_sectors: usize,
    slot: u8,
    phys_part_idx: u8,
    start_sector: u64,
) -> anyhow::Result<()> {
    let mut bytes_left = num_sectors * channel.fh_config().storage_sector_size;
    let mut xml = firehose_xml_setup(
        "read",
        &[
            (
                "SECTOR_SIZE_IN_BYTES",
                &channel.fh_config().storage_sector_size.to_string(),
            ),
            ("num_partition_sectors", &num_sectors.to_string()),
            ("slot", &slot.to_string()),
            ("physical_partition_number", &phys_part_idx.to_string()),
            ("start_sector", &start_sector.to_string()),
        ],
    )?;

    firehose_write(channel, &mut xml)?;
    if firehose_read(channel, firehose_parser_ack_nak)? != FirehoseStatus::Ack {
        bail!("Read request was NAKed");
    }

    let mut pb = ProgressBar::new(bytes_left as u64);
    pb.set_units(Units::Bytes);

    let mut last_read_was_zero_len = false;
    while bytes_left > 0 {
        let chunk_size_bytes = min(bytes_left, channel.fh_config().recv_buffer_size);
        let mut buf = vec![0; chunk_size_bytes];

        let n = channel.read(&mut buf).context("Error receiving data")?;
        if n == 0 {
            // TODO: need more robustness here
            /* Every 2 or 3 packets should be empty? */
            last_read_was_zero_len = true;
            continue;
        }

        last_read_was_zero_len = false;
        out.write_all(&buf[..n])
            .context("Error writing storage data")?;

        bytes_left -= n;
        pb.add(n as u64);
    }
    // Close the bar with a newline. `pbr` only ever rewrites its single row
    // with `\r`, so without this the final 100% state stays on the current
    // line and the next log message is appended to it.
    pb.finish_println("");

    if !last_read_was_zero_len && channel.fh_config().backend == QdlBackend::Usb {
        // Issue a dummy read to drain the queue
        let _ = channel.read(&mut [])?;
    }

    if firehose_read(channel, firehose_parser_ack_nak)? != FirehoseStatus::Ack {
        bail!("Failed to complete 'read' op");
    }

    Ok(())
}

/// Reboot or power off the Device
pub fn firehose_reset<T: QdlChan>(
    channel: &mut T,
    mode: &FirehoseResetMode,
    delay_in_sec: u32,
) -> anyhow::Result<()> {
    let mut xml = firehose_xml_setup(
        "power",
        &[
            (
                "value",
                match mode {
                    FirehoseResetMode::ResetToEdl => "reset_to_edl",
                    FirehoseResetMode::Reset => "reset",
                    FirehoseResetMode::Off => "off",
                },
            ),
            ("DelayInSeconds", &delay_in_sec.to_string()),
        ],
    )?;

    firehose_write_getack(channel, &mut xml, "reset the Device".to_owned())?;

    // Drain the incoming LOG packets to actually restart the device
    let _ = channel.skip_until(0);

    Ok(())
}

/// Mark a physical storage partition as bootable
pub fn firehose_set_bootable<T: QdlChan>(channel: &mut T, drive_idx: u8) -> anyhow::Result<()> {
    let mut xml = firehose_xml_setup(
        "setbootablestoragedrive",
        &[("value", &drive_idx.to_string())],
    )?;

    firehose_write_getack(
        channel,
        &mut xml,
        format!("set partition {drive_idx} as bootable"),
    )
}

pub fn firehose_get_default_sector_size(t: &str) -> Option<usize> {
    match FirehoseStorageType::from_str(t).ok()? {
        FirehoseStorageType::Emmc => Some(512),
        FirehoseStorageType::Nand => Some(4096),
        FirehoseStorageType::Nvme => Some(512),
        FirehoseStorageType::Ufs => Some(4096),
        FirehoseStorageType::Spinor => Some(4096),
    }
}
