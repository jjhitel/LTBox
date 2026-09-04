//! Extraction of the AVB public key embedded in Qualcomm ABL images.

use std::{collections::HashSet, io::Read, path::Path};

use ltbox_core::LtboxError;
use lzma_rust2::LzmaReader;
use sha1::{Digest, Sha1};

const LZMA_OFFSET: usize = 0x1078;
const MAX_DECOMPRESSED_SIZE: usize = 64 * 1024 * 1024;
const MAX_INPUT_SIZE: usize = MAX_DECOMPRESSED_SIZE * 4;
const RSA_KEY_BITS: [u32; 3] = [2048, 4096, 8192];

#[derive(Clone, Debug)]
struct Section {
    name: String,
    virtual_address: u32,
    raw_offset: usize,
    raw_size: usize,
    executable: bool,
}

#[derive(Debug)]
struct PeImage {
    data: Vec<u8>,
    sections: Vec<Section>,
}

impl PeImage {
    fn raw_to_rva(&self, raw_offset: usize) -> Option<i64> {
        self.sections.iter().find_map(|section| {
            let end = section.raw_offset.checked_add(section.raw_size)?;
            (section.raw_offset <= raw_offset && raw_offset < end).then(|| {
                i64::from(section.virtual_address) + (raw_offset - section.raw_offset) as i64
            })
        })
    }

    fn text_ranges(&self) -> Vec<(usize, usize, i64)> {
        let named: Vec<_> = self
            .sections
            .iter()
            .filter(|section| section.name == ".text")
            .collect();
        let executable: Vec<_> = self
            .sections
            .iter()
            .filter(|section| section.executable)
            .collect();
        let selected = if !named.is_empty() {
            named
        } else if !executable.is_empty() {
            executable
        } else {
            self.sections.iter().collect()
        };

        selected
            .into_iter()
            .filter_map(|section| {
                if section.raw_size == 0 || section.raw_offset >= self.data.len() {
                    return None;
                }
                let end = section
                    .raw_offset
                    .saturating_add(section.raw_size)
                    .min(self.data.len());
                Some((section.raw_offset, end, i64::from(section.virtual_address)))
            })
            .collect()
    }
}

#[derive(Debug)]
struct KeyBlob {
    raw_offset: usize,
    blob: Vec<u8>,
    references: usize,
}

/// Extract the AVB public-key SHA-1 (lowercase hex) embedded in an ABL ELF.
pub fn extract_abl_avb_pubkey_sha1(path: &Path) -> Result<String, LtboxError> {
    // An ABL image is a few hundred KiB. Refuse anything that could not hold a
    // decodable payload anyway rather than reading it into memory first.
    let size = fs_err::metadata(path)?.len();
    if size > MAX_INPUT_SIZE as u64 {
        return Err(LtboxError::Avb(format!(
            "ABL image is too large to inspect ({size} bytes)"
        )));
    }
    let raw = fs_err::read(path)?;
    extract_from_bytes(&raw)
}

fn extract_from_bytes(raw: &[u8]) -> Result<String, LtboxError> {
    let pe = load_linuxloader(raw)
        .ok_or_else(|| LtboxError::Avb("ABL LinuxLoader EFI not found".to_string()))?;
    let mut keys = find_key_blobs(&pe);
    let highest = keys
        .iter()
        .map(|key| key.references)
        .max()
        .ok_or_else(|| LtboxError::Avb("ABL AVB public-key blob not found".to_string()))?;
    keys.retain(|key| key.references == highest);
    keys.sort_unstable_by_key(|key| key.raw_offset);
    let key = keys
        .first()
        .ok_or_else(|| LtboxError::Avb("ABL AVB public-key blob not found".to_string()))?;
    let digest = Sha1::digest(&key.blob);
    let mut fingerprint = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        fingerprint.push(char::from(HEX[usize::from(byte >> 4)]));
        fingerprint.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(fingerprint)
}

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    let bytes: [u8; 8] = data.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn read_u32_be(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

fn lzma_decode(stream: &[u8]) -> Option<Vec<u8>> {
    if stream.is_empty() || stream.len() > MAX_INPUT_SIZE {
        return None;
    }

    let mut spliced = Vec::new();
    let mut attempts = vec![stream];
    if stream.len() >= 5 && stream.first() == Some(&0x5d) {
        spliced.reserve(stream.len().checked_add(8)?);
        spliced.extend_from_slice(stream.get(..5)?);
        spliced.extend_from_slice(&[0xff; 8]);
        spliced.extend_from_slice(stream.get(5..)?);
        attempts.push(&spliced);
    }

    for candidate in attempts {
        if candidate.len() < 13 {
            continue;
        }
        let declared_size = read_u64_le(candidate, 5)?;
        if declared_size != u64::MAX && declared_size > MAX_DECOMPRESSED_SIZE as u64 {
            continue;
        }

        let mut reader =
            match LzmaReader::new_mem_limit(candidate, (MAX_DECOMPRESSED_SIZE / 1024) as u32, None)
            {
                Ok(reader) => reader,
                Err(_) => continue,
            };
        let mut decoded = Vec::new();
        match reader
            .by_ref()
            .take((MAX_DECOMPRESSED_SIZE + 1) as u64)
            .read_to_end(&mut decoded)
        {
            Ok(_) if (64..=MAX_DECOMPRESSED_SIZE).contains(&decoded.len()) => return Some(decoded),
            _ => continue,
        }
    }
    None
}

fn decompressed_layers(raw: &[u8]) -> Vec<Vec<u8>> {
    let mut starts = Vec::new();
    if raw.len() > LZMA_OFFSET {
        starts.push(LZMA_OFFSET);
    }
    for (offset, window) in raw.windows(3).enumerate() {
        if window == [0x5d, 0, 0] {
            starts.push(offset);
        }
    }

    let mut seen = HashSet::new();
    starts
        .into_iter()
        .filter(|start| seen.insert(*start))
        .filter_map(|start| lzma_decode(raw.get(start..)?))
        .filter(|layer| !find_pe_images(layer).is_empty())
        .collect()
}

fn find_pe_images(data: &[u8]) -> Vec<PeImage> {
    let mut images = Vec::new();
    let mut cursor = 0usize;

    while cursor < data.len() {
        let Some(relative_mz) = data
            .get(cursor..)
            .and_then(|tail| tail.windows(2).position(|window| window == b"MZ"))
        else {
            break;
        };
        let Some(mz) = cursor.checked_add(relative_mz) else {
            break;
        };
        cursor = match mz.checked_add(2) {
            Some(next) => next,
            None => break,
        };

        let Some(dos_end) = mz.checked_add(0x40) else {
            continue;
        };
        if dos_end > data.len() {
            continue;
        }
        let Some(e_lfanew) = read_u32_le(data, mz + 0x3c).map(|value| value as usize) else {
            continue;
        };
        let Some(pe_header) = mz.checked_add(e_lfanew) else {
            continue;
        };
        let Some(pe_fixed_end) = pe_header.checked_add(24) else {
            continue;
        };
        if pe_header < mz || pe_fixed_end > data.len() {
            continue;
        }
        if data.get(pe_header..pe_header + 4) != Some(b"PE\0\0".as_slice()) {
            continue;
        }

        let Some(number_of_sections) = read_u16_le(data, pe_header + 6).map(usize::from) else {
            continue;
        };
        let Some(optional_size) = read_u16_le(data, pe_header + 20).map(usize::from) else {
            continue;
        };
        if !(1..=96).contains(&number_of_sections) || optional_size < 0x60 {
            continue;
        }
        let optional = pe_header + 24;
        let Some(optional_end) = optional.checked_add(optional_size) else {
            continue;
        };
        if optional_end > data.len() || !matches!(read_u16_le(data, optional), Some(0x10b | 0x20b))
        {
            continue;
        }
        let section_table = optional_end;
        let Some(table_size) = number_of_sections.checked_mul(40) else {
            continue;
        };
        let Some(table_end) = section_table.checked_add(table_size) else {
            continue;
        };
        if table_end > data.len() {
            continue;
        }

        let Some(size_of_headers) = read_u32_le(data, optional + 60).map(|value| value as usize)
        else {
            continue;
        };
        let mut real_size = size_of_headers.max(table_end - mz);
        let mut sections = Vec::with_capacity(number_of_sections);
        let mut valid = true;
        for index in 0..number_of_sections {
            let Some(section) = section_table.checked_add(index * 40) else {
                valid = false;
                break;
            };
            let Some(raw_size) = read_u32_le(data, section + 16).map(|value| value as usize) else {
                valid = false;
                break;
            };
            let Some(raw_offset) = read_u32_le(data, section + 20).map(|value| value as usize)
            else {
                valid = false;
                break;
            };
            let Some(virtual_address) = read_u32_le(data, section + 12) else {
                valid = false;
                break;
            };
            let Some(characteristics) = read_u32_le(data, section + 36) else {
                valid = false;
                break;
            };
            let Some(raw_end) = raw_offset.checked_add(raw_size) else {
                valid = false;
                break;
            };
            if raw_size != 0 && raw_end > data.len() - mz {
                valid = false;
                break;
            }
            real_size = real_size.max(raw_end);
            let Some(name_bytes) = data.get(section..section + 8) else {
                valid = false;
                break;
            };
            let name_end = name_bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name_bytes.len());
            sections.push(Section {
                name: String::from_utf8_lossy(&name_bytes[..name_end]).into_owned(),
                virtual_address,
                raw_offset,
                raw_size,
                executable: characteristics & 0x2000_0000 != 0,
            });
        }
        let Some(image_end) = mz.checked_add(real_size) else {
            continue;
        };
        if !valid || real_size == 0 || image_end > data.len() {
            continue;
        }
        let Some(image_data) = data.get(mz..image_end) else {
            continue;
        };
        images.push(PeImage {
            data: image_data.to_vec(),
            sections,
        });
    }
    images
}

fn load_linuxloader(raw: &[u8]) -> Option<PeImage> {
    let images = if raw.starts_with(b"MZ") {
        find_pe_images(raw)
    } else {
        decompressed_layers(raw)
            .iter()
            .flat_map(|layer| find_pe_images(layer))
            .collect()
    };
    images.into_iter().max_by_key(|image| image.data.len())
}

fn bit_length_be(value: &[u8]) -> usize {
    let Some((index, first)) = value.iter().enumerate().find(|(_, byte)| **byte != 0) else {
        return 0;
    };
    (value.len() - index - 1) * 8 + (8 - first.leading_zeros() as usize)
}

fn be_bytes_to_limbs(value: &[u8], limb_count: usize) -> Vec<u32> {
    let mut limbs = vec![0u32; limb_count];
    for (index, chunk) in value.rchunks(4).enumerate().take(limb_count) {
        let mut bytes = [0u8; 4];
        bytes[4 - chunk.len()..].copy_from_slice(chunk);
        limbs[index] = u32::from_be_bytes(bytes);
    }
    limbs
}

fn limbs_cmp(left: &[u32], right: &[u32]) -> std::cmp::Ordering {
    left.iter().rev().cmp(right.iter().rev())
}

fn limbs_sub_assign(left: &mut [u32], right: &[u32]) {
    let mut borrow = 0u64;
    for (left_limb, right_limb) in left.iter_mut().zip(right) {
        let subtrahend = u64::from(*right_limb) + borrow;
        let minuend = u64::from(*left_limb);
        *left_limb = minuend.wrapping_sub(subtrahend) as u32;
        borrow = u64::from(minuend < subtrahend);
    }
}

fn montgomery_rr_matches(modulus_be: &[u8], rr_be: &[u8], modulus_bits: usize) -> bool {
    let limb_count = modulus_be.len().div_ceil(4);
    let modulus = be_bytes_to_limbs(modulus_be, limb_count);
    let expected = be_bytes_to_limbs(rr_be, limb_count);
    let mut value = vec![0u32; limb_count];
    value[0] = 1;

    for _ in 0..modulus_bits.saturating_mul(2) {
        let mut carry = 0u32;
        for limb in &mut value {
            let next = *limb >> 31;
            *limb = (*limb << 1) | carry;
            carry = next;
        }
        if carry != 0 || limbs_cmp(&value, &modulus).is_ge() {
            limbs_sub_assign(&mut value, &modulus);
        }
    }
    value == expected
}

fn decode_key_candidate(data: &[u8], offset: usize, bits: u32) -> Option<Vec<u8>> {
    let modulus_size = usize::try_from(bits / 8).ok()?;
    let size = 8usize.checked_add(modulus_size.checked_mul(2)?)?;
    let blob = data.get(offset..offset.checked_add(size)?)?;
    if read_u32_be(blob, 0)? != bits {
        return None;
    }
    let n0inv = read_u32_be(blob, 4)?;
    let modulus_end = 8usize.checked_add(modulus_size)?;
    let modulus = blob.get(8..modulus_end)?;
    let rr = blob.get(modulus_end..size)?;
    let modulus_bits = bit_length_be(modulus);
    if !(bits as usize / 2 < modulus_bits && modulus_bits <= bits as usize)
        || modulus.last().is_none_or(|byte| byte & 1 == 0)
    {
        return None;
    }

    let low_word = read_u32_be(modulus, modulus_size - 4)?;
    let mut inverse = low_word;
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u32.wrapping_sub(low_word.wrapping_mul(inverse)));
    }
    if n0inv != inverse.wrapping_neg() || !montgomery_rr_matches(modulus, rr, modulus_bits) {
        return None;
    }
    Some(blob.to_vec())
}

fn sign_extend(value: u32, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((i64::from(value)) << shift) >> shift
}

fn instruction_references(pe: &PeImage, key_start: usize, key_size: usize) -> usize {
    let Some(key_rva) = pe.raw_to_rva(key_start) else {
        return 0;
    };
    let Some(key_end) = key_start.checked_add(key_size) else {
        return 0;
    };
    let Some(key_rva_end) = key_rva.checked_add(key_size as i64) else {
        return 0;
    };
    let mut references = 0usize;

    for (raw_start, raw_end, section_rva) in pe.text_ranges() {
        let Some(first) = raw_start.checked_add((4 - (raw_start & 3)) & 3) else {
            continue;
        };
        let mut instruction = first;
        while instruction.checked_add(4).is_some_and(|end| end <= raw_end) {
            if key_start <= instruction && instruction < key_end {
                instruction += 4;
                continue;
            }
            let Some(word) = read_u32_le(&pe.data, instruction) else {
                break;
            };
            let pc = section_rva + (instruction - raw_start) as i64;

            if word & 0x9f00_0000 == 0x1000_0000 {
                let immlo = (word >> 29) & 3;
                let immhi = (word >> 5) & 0x7ffff;
                let target = pc + sign_extend((immhi << 2) | immlo, 21);
                if key_rva <= target && target < key_rva_end {
                    references += 1;
                }
                instruction += 4;
                continue;
            }

            if word & 0x9f00_0000 == 0x9000_0000 {
                let immlo = (word >> 29) & 3;
                let immhi = (word >> 5) & 0x7ffff;
                let page = (pc & !0xfff) + (sign_extend((immhi << 2) | immlo, 21) << 12);
                let register = word & 0x1f;
                let add_limit = instruction.saturating_add(20).min(raw_end);
                let mut add_offset = instruction + 4;
                while add_offset < add_limit {
                    let Some(add) = read_u32_le(&pe.data, add_offset) else {
                        break;
                    };
                    if add & 0x7f00_0000 == 0x1100_0000
                        && add & 0x8000_0000 != 0
                        && (add >> 5) & 0x1f == register
                    {
                        let mut immediate = i64::from((add >> 10) & 0xfff);
                        if (add >> 22) & 1 != 0 {
                            immediate <<= 12;
                        }
                        let target = page + immediate;
                        if key_rva <= target && target < key_rva_end {
                            references += 1;
                        }
                        break;
                    }
                    add_offset += 4;
                }
            }
            instruction += 4;
        }
    }
    references
}

fn find_key_blobs(pe: &PeImage) -> Vec<KeyBlob> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for bits in RSA_KEY_BITS {
        let marker = bits.to_be_bytes();
        for offset in pe
            .data
            .windows(marker.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == marker).then_some(offset))
        {
            let Some(blob) = decode_key_candidate(&pe.data, offset, bits) else {
                continue;
            };
            if pe.raw_to_rva(offset).is_none() || !seen.insert(blob.clone()) {
                continue;
            }
            let references = instruction_references(pe, offset, blob.len());
            found.push(KeyBlob {
                raw_offset: offset,
                blob,
                references,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs, io::Write, path::PathBuf};

    use lzma_rust2::{LzmaOptions, LzmaWriter};

    use super::{extract_abl_avb_pubkey_sha1, extract_from_bytes, lzma_decode};

    #[test]
    fn garbage_and_truncated_inputs_return_errors() {
        let garbage: Vec<u8> = (0..8192)
            .map(|index| ((index * 73 + 41) & 0xff) as u8)
            .collect();
        for input in [&[][..], b"MZ", &garbage[..], &garbage[..63]] {
            assert!(extract_from_bytes(input).is_err());
        }
    }

    #[test]
    fn lzma_alone_unknown_size_and_headerless_size_decode() {
        let payload: Vec<u8> = (0..256).map(|value| value as u8).collect();
        let mut writer =
            LzmaWriter::new_use_header(Vec::new(), &LzmaOptions::default(), None).unwrap();
        writer.write_all(&payload).unwrap();
        let encoded = writer.finish().unwrap();
        assert_eq!(&encoded[5..13], &[0xff; 8]);
        assert_eq!(lzma_decode(&encoded).as_deref(), Some(payload.as_slice()));

        let mut without_size = encoded[..5].to_vec();
        without_size.extend_from_slice(&encoded[13..]);
        assert_eq!(
            lzma_decode(&without_size).as_deref(),
            Some(payload.as_slice())
        );
    }

    #[test]
    #[ignore = "needs LTBOX_TEST_ABL_DIR=/path/to/abl/fixtures"]
    fn extracts_local_abl_fixtures() {
        let directory = PathBuf::from(
            std::env::var_os("LTBOX_TEST_ABL_DIR")
                .expect("LTBOX_TEST_ABL_DIR must point to the ABL fixture directory"),
        );
        let expected = HashMap::from([
            ("abl_tb320.elf", "2597c218aae470a130f61162feaae70afd97f011"),
            ("abl_tb321.elf", "2597c218aae470a130f61162feaae70afd97f011"),
            (
                "abl_tb322_new.elf",
                "8fcb864f11f53ed11284615fb67685522085d3a2",
            ),
            (
                "abl_tb322_old.elf",
                "2597c218aae470a130f61162feaae70afd97f011",
            ),
            ("abl_tb520.elf", "2597c218aae470a130f61162feaae70afd97f011"),
            ("abl_tb710.elf", "2597c218aae470a130f61162feaae70afd97f011"),
            (
                "abl_tb710_new.elf",
                "2597c218aae470a130f61162feaae70afd97f011",
            ),
        ]);

        let mut paths: Vec<_> = fs::read_dir(&directory)
            .expect("read LTBOX_TEST_ABL_DIR")
            .map(|entry| entry.expect("read fixture directory entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "elf"))
            .collect();
        paths.sort();
        assert!(
            !paths.is_empty(),
            "LTBOX_TEST_ABL_DIR contains no *.elf files"
        );

        let mut fingerprints = std::collections::HashSet::new();
        for path in paths {
            let fingerprint = extract_abl_avb_pubkey_sha1(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            println!(
                "{} {fingerprint}",
                path.file_name().unwrap().to_string_lossy()
            );
            fingerprints.insert(fingerprint.clone());
            if let Some(expected) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| expected.get(name))
            {
                assert_eq!(&fingerprint, expected, "{}", path.display());
            }
        }
        assert_eq!(fingerprints.len(), 2);
        assert!(fingerprints.contains("2597c218aae470a130f61162feaae70afd97f011"));
        assert!(fingerprints.contains("8fcb864f11f53ed11284615fb67685522085d3a2"));
    }
}
