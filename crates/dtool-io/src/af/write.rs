use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use dtool_core::Document;
use crate::IoError;
use crate::af::container::{AfContainer, AfEntry, AfCompression};

pub fn save_af(doc: &Document, path: &Path) -> Result<(), IoError> {
    let tmp_path = path.with_extension("tmp");
    
    let file = fs::File::create(&tmp_path).map_err(|e| IoError::from_io(&e))?;
    let mut writer = BufWriter::new(file);

    let doc_bytes = serialize_doc_dat(doc)?;
    let entries = build_container_entries(&doc_bytes);
    
    write_container_header(&mut writer, &entries)?;
    write_container_data(&mut writer, &entries, &doc_bytes)?;
    
    writer.flush().map_err(|e| IoError::from_io(&e))?;
    
    fs::rename(&tmp_path, path).map_err(|e| IoError::from_io(&e))?;

    Ok(())
}

fn serialize_doc_dat(doc: &Document) -> Result<Vec<u8>, IoError> {
    let mut buf = Vec::new();
    write_node_to_buffer(&mut buf, doc)?;
    Ok(buf)
}

fn write_node_to_buffer(buf: &mut Vec<u8>, doc: &Document) -> Result<(), IoError> {
    buf.extend_from_slice(b"DOC");
    buf.push(0x01); 
    let len = 0u32; 
    buf.extend_from_slice(&len.to_le_bytes());
    Ok(())
}

fn build_container_entries(doc_bytes: &[u8]) -> Vec<AfEntry> {
    vec![AfEntry {
        name: "doc.dat".to_string(),
        offset: 0,
        size: doc_bytes.len() as u64,
        stored_size: doc_bytes.len() as u64,
        crc32: calculate_crc32(doc_bytes),
        compression: AfCompression::None,
    }]
}

fn write_container_header(writer: &mut BufWriter<fs::File>, entries: &[AfEntry]) -> Result<(), IoError> {
    let magic: u32 = 0x4146494E; 
    let version = 3u16;
    let flags = 0u32;
    let revision = 0u32;
    
    writer.write_all(&magic.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    writer.write_all(&version.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    writer.write_all(&flags.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    writer.write_all(&revision.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    
    let entry_count = entries.len() as u32;
    writer.write_all(&entry_count.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    
    for entry in entries {
        write_entry_header(writer, entry)?;
    }
    
    Ok(())
}

fn write_entry_header(writer: &mut BufWriter<fs::File>, entry: &AfEntry) -> Result<(), IoError> {
    let name_len = entry.name.len() as u32;
    writer.write_all(&name_len.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    writer.write_all(entry.name.as_bytes()).map_err(|e| IoError::from_io(&e))?;
    
    writer.write_all(&entry.offset.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    writer.write_all(&entry.size.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    writer.write_all(&entry.stored_size.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    writer.write_all(&entry.crc32.to_le_bytes()).map_err(|e| IoError::from_io(&e))?;
    
    let comp_val = match entry.compression {
        AfCompression::None => 0u8,
        AfCompression::Zlib => 1u8,
        AfCompression::Zstd => 2u8,
    };
    writer.write_all(&[comp_val]).map_err(|e| IoError::from_io(&e))?;
    
    Ok(())
}

fn write_container_data(writer: &mut BufWriter<fs::File>, entries: &[AfEntry], doc_bytes: &[u8]) -> Result<(), IoError> {
    for entry in entries {
        if entry.name == "doc.dat" {
            writer.write_all(doc_bytes).map_err(|e| IoError::from_io(&e))?;
        }
    }
    Ok(())
}

fn calculate_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}
