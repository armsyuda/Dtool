use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::ZlibDecoder;
use zstd::stream::decode_all;

use crate::{IoError, IoErrorKind};

fn map_fs_error(err: std::io::Error) -> IoError {
    match err.kind() {
        std::io::ErrorKind::NotFound => IoError::new(IoErrorKind::NotFound),
        std::io::ErrorKind::PermissionDenied => IoError::new(IoErrorKind::Permission),
        _ => IoError::from_io(&err),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfCompression {
    None,
    Zlib,
    Zstd,
}

#[derive(Debug, Clone)]
pub struct AfEntry {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    pub stored_size: u64,
    pub crc32: u32,
    pub compression: AfCompression,
}

#[derive(Debug, Clone)]
pub struct AfContainer {
    pub magic: u32,
    pub version: u16,
    pub flags: u32,
    pub revision: u32,
    pub entries: Vec<AfEntry>,
}

fn read_u32_le(data: &[u8], offset: usize) -> Result<u32, IoError> {
    if offset + 4 > data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    Ok(u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]))
}

fn read_u64_le(data: &[u8], offset: usize) -> Result<u64, IoError> {
    if offset + 8 > data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    Ok(u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

fn read_u16_le(data: &[u8], offset: usize) -> Result<u16, IoError> {
    if offset + 2 > data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

fn read_u8(data: &[u8], offset: usize) -> Result<u8, IoError> {
    if offset >= data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    Ok(data[offset])
}

pub fn read_container(path: &Path) -> Result<AfContainer, IoError> {
    let mut file = File::open(path).map_err(map_fs_error)?;
    if file.metadata().map_err(map_fs_error)?.len() < 16 {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    let mut header_buf = [0u8; 16];
    file.read_exact(&mut header_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;

    let magic = read_u32_le(&header_buf, 0)?;
    if magic != 0x496E666E {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    let version = read_u16_le(&header_buf, 4)?;
    let flags = read_u32_le(&header_buf, 6)?;
    let revision = read_u32_le(&header_buf, 10)?;

    let mut entries: Vec<AfEntry> = Vec::new();

    if version >= 8 {
        let mut torp_buf = [0u8; 4];
        file.read_exact(&mut torp_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
        
        if &torp_buf != b"torP" {
            return Err(IoError::new(IoErrorKind::InvalidFormat));
        }

        let mut fat_size_buf = [0u8; 4];
        file.read_exact(&mut fat_size_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
        let fat_size = read_u32_le(&fat_size_buf, 0)? as usize;

        let mut fat_data = vec![0u8; fat_size];
        file.read_exact(&mut fat_data).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;

        let mut pos = 0;
        while pos < fat_data.len() {
            if pos + 4 > fat_data.len() {
                break;
            }
            
            let name_len = read_u8(&fat_data, pos)? as usize;
            pos += 1;

            if pos + name_len > fat_data.len() {
                return Err(IoError::new(IoErrorKind::InvalidFormat));
            }

            let name_bytes = &fat_data[pos..pos + name_len];
            let name = String::from_utf8_lossy(name_bytes).to_string();
            pos += name_len;

            if pos + 24 > fat_data.len() {
                return Err(IoError::new(IoErrorKind::InvalidFormat));
            }

            let offset = read_u64_le(&fat_data, pos)?;
            pos += 8;

            let stored_size = read_u64_le(&fat_data, pos)?;
            pos += 8;

            let size = read_u64_le(&fat_data, pos)?;
            pos += 8;

            let crc32 = read_u32_le(&fat_data, pos)?;
            pos += 4;

            let compression_byte = read_u8(&fat_data, pos)?;
            pos += 1;

            let compression = match compression_byte {
                0 => AfCompression::None,
                1 => AfCompression::Zlib,
                2 => AfCompression::Zstd,
                _ => return Err(IoError::new(IoErrorKind::InvalidFormat)),
            };

            entries.push(AfEntry {
                name,
                offset,
                size,
                stored_size,
                crc32,
                compression,
            });
        }
    } else {
        let mut fni_buf = [0u8; 4];
        file.read_exact(&mut fni_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;

        if &fni_buf != b"fnI#" {
            return Err(IoError::new(IoErrorKind::InvalidFormat));
        }

        let mut entry_count_buf = [0u8; 4];
        file.read_exact(&mut entry_count_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
        let entry_count = read_u32_le(&entry_count_buf, 0)? as usize;

        for _ in 0..entry_count {
            let mut name_len_buf = [0u8; 1];
            file.read_exact(&mut name_len_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            let name_len = name_len_buf[0] as usize;

            if name_len == 0 {
                return Err(IoError::new(IoErrorKind::InvalidFormat));
            }

            let mut name_bytes = vec![0u8; name_len];
            file.read_exact(&mut name_bytes).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            let name = String::from_utf8_lossy(&name_bytes).to_string();

            let mut offset_buf = [0u8; 8];
            file.read_exact(&mut offset_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            let offset = read_u64_le(&offset_buf, 0)?;

            let mut stored_size_buf = [0u8; 8];
            file.read_exact(&mut stored_size_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            let stored_size = read_u64_le(&stored_size_buf, 0)?;

            let mut size_buf = [0u8; 8];
            file.read_exact(&mut size_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            let size = read_u64_le(&size_buf, 0)?;

            let mut crc32_buf = [0u8; 4];
            file.read_exact(&mut crc32_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            let crc32 = read_u32_le(&crc32_buf, 0)?;

            let mut compression_buf = [0u8; 1];
            file.read_exact(&mut compression_buf).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            let compression_byte = compression_buf[0];

            let compression = match compression_byte {
                0 => AfCompression::None,
                1 => AfCompression::Zlib,
                2 => AfCompression::Zstd,
                _ => return Err(IoError::new(IoErrorKind::InvalidFormat)),
            };

            entries.push(AfEntry {
                name,
                offset,
                size,
                stored_size,
                crc32,
                compression,
            });
        }
    }

    Ok(AfContainer {
        magic,
        version,
        flags,
        revision,
        entries,
    })
}

pub fn read_entry(path: &Path, entry: &AfEntry) -> Result<Vec<u8>, IoError> {
    let mut file = File::open(path).map_err(map_fs_error)?;
    
    let file_size = file.metadata().map_err(map_fs_error)?.len();
    
    if entry.offset + entry.stored_size > file_size {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    file.seek(SeekFrom::Start(entry.offset)).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;

    let mut stored_data = vec![0u8; entry.stored_size as usize];
    file.read_exact(&mut stored_data).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;

    let decompressed = match entry.compression {
        AfCompression::None => stored_data,
        AfCompression::Zlib => {
            let mut decoder = ZlibDecoder::new(&stored_data[..]);
            let mut out = Vec::with_capacity(entry.size as usize);
            decoder.read_to_end(&mut out).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?;
            out
        }
        AfCompression::Zstd => {
            decode_all(&stored_data[..]).map_err(|e| IoError::new(IoErrorKind::InvalidFormat))?
        }
    };

    if decompressed.len() as u64 != entry.size {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    if entry.crc32 != 0 {
        let computed_crc = crc32_compute(&decompressed);
        if computed_crc != entry.crc32 {
            return Err(IoError::new(IoErrorKind::InvalidFormat));
        }
    }

    Ok(decompressed)
}

fn crc32_compute(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256 {
        let mut c = i as u32;
        for _ in 0..8 {
            if c & 1 != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
        }
        table[i] = c;
    }

    let mut crc = 0xFFFFFFFFu32;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFFFFFFu32
}

