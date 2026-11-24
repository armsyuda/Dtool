use crate::{IoError, IoErrorKind};

#[derive(Debug, Clone)]
pub struct AfNode {
    pub tag: String,
    pub fields: Vec<(String, AfValue)>,
    pub children: Vec<AfNode>,
}

#[derive(Debug, Clone)]
pub enum AfValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
    Node(Box<AfNode>),
    List(Vec<AfValue>),
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

fn read_f64_le(data: &[u8], offset: usize) -> Result<f64, IoError> {
    if offset + 8 > data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    Ok(f64::from_le_bytes([
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

fn read_string(data: &[u8], offset: usize) -> Result<(String, usize), IoError> {
    let mut offset = offset;
    if offset >= data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    
    let len = data[offset] as usize;
    offset += 1;

    if offset + len > data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    let s = std::str::from_utf8(&data[offset..offset + len])
        .map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?
        .to_string();
    Ok((s, offset + len))
}

fn parse_node(data: &[u8], pos: &mut usize) -> Result<AfNode, IoError> {
    if *pos >= data.len() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    let (tag, new_pos) = read_string(data, *pos)?;
    *pos = new_pos;

    let mut fields: Vec<(String, AfValue)> = Vec::new();
    let mut children: Vec<AfNode> = Vec::new();

    if *pos >= data.len() {
        return Ok(AfNode { tag, fields, children });
    }

    let field_count = data[*pos] as usize;
    *pos += 1;

    for _ in 0..field_count {
        if *pos >= data.len() {
            break;
        }

        let (key, new_pos) = read_string(data, *pos)?;
        *pos = new_pos;

        if *pos >= data.len() {
            return Err(IoError::new(IoErrorKind::InvalidFormat));
        }

        let type_byte = data[*pos];
        *pos += 1;

        let value = match type_byte {
            0 => {
                if *pos + 8 > data.len() {
                    return Err(IoError::new(IoErrorKind::InvalidFormat));
                }
                let val = read_u64_le(data, *pos)? as i64;
                *pos += 8;
                AfValue::Int(val)
            }
            1 => {
                if *pos + 8 > data.len() {
                    return Err(IoError::new(IoErrorKind::InvalidFormat));
                }
                let val = read_f64_le(data, *pos)?;
                *pos += 8;
                AfValue::Float(val)
            }
            2 => {
                if *pos >= data.len() {
                    return Err(IoError::new(IoErrorKind::InvalidFormat));
                }
                let val = data[*pos] != 0;
                *pos += 1;
                AfValue::Bool(val)
            }
            3 => {
                let (val, new_pos) = read_string(data, *pos)?;
                *pos = new_pos;
                AfValue::Text(val)
            }
            4 => {
                if *pos + 4 > data.len() {
                    return Err(IoError::new(IoErrorKind::InvalidFormat));
                }
                let len = read_u32_le(data, *pos)? as usize;
                *pos += 4;

                if *pos + len > data.len() {
                    return Err(IoError::new(IoErrorKind::InvalidFormat));
                }

                let val = data[*pos..*pos + len].to_vec();
                *pos += len;
                AfValue::Bytes(val)
            }
            5 => {
                let child_node = parse_node(data, pos)?;
                AfValue::Node(Box::new(child_node))
            }
            6 => {
                if *pos + 4 > data.len() {
                    return Err(IoError::new(IoErrorKind::InvalidFormat));
                }
                let count = read_u32_le(data, *pos)? as usize;
                *pos += 4;

                let mut list = Vec::with_capacity(count);
                for _ in 0..count {
                    if *pos >= data.len() {
                        break;
                    }
                    let item_type = data[*pos];
                    *pos += 1;

                    let item_value = match item_type {
                        0 => {
                            if *pos + 8 > data.len() {
                                return Err(IoError::new(IoErrorKind::InvalidFormat));
                            }
                            let val = read_u64_le(data, *pos)? as i64;
                            *pos += 8;
                            AfValue::Int(val)
                        }
                        1 => {
                            if *pos + 8 > data.len() {
                                return Err(IoError::new(IoErrorKind::InvalidFormat));
                            }
                            let val = read_f64_le(data, *pos)?;
                            *pos += 8;
                            AfValue::Float(val)
                        }
                        2 => {
                            if *pos >= data.len() {
                                return Err(IoError::new(IoErrorKind::InvalidFormat));
                            }
                            let val = data[*pos] != 0;
                            *pos += 1;
                            AfValue::Bool(val)
                        }
                        3 => {
                            let (val, new_pos) = read_string(data, *pos)?;
                            *pos = new_pos;
                            AfValue::Text(val)
                        }
                        4 => {
                            if *pos + 4 > data.len() {
                                return Err(IoError::new(IoErrorKind::InvalidFormat));
                            }
                            let len = read_u32_le(data, *pos)? as usize;
                            *pos += 4;

                            if *pos + len > data.len() {
                                return Err(IoError::new(IoErrorKind::InvalidFormat));
                            }

                            let val = data[*pos..*pos + len].to_vec();
                            *pos += len;
                            AfValue::Bytes(val)
                        }
                        5 => {
                            let child_node = parse_node(data, pos)?;
                            AfValue::Node(Box::new(child_node))
                        }
                        _ => return Err(IoError::new(IoErrorKind::InvalidFormat)),
                    };

                    list.push(item_value);
                }
                AfValue::List(list)
            }
            _ => return Err(IoError::new(IoErrorKind::InvalidFormat)),
        };

        fields.push((key, value));
    }

    if *pos < data.len() {
        let child_count = data[*pos] as usize;
        *pos += 1;

        for _ in 0..child_count {
            if *pos >= data.len() {
                break;
            }
            let child_node = parse_node(data, pos)?;
            children.push(child_node);
        }
    }

    Ok(AfNode { tag, fields, children })
}

pub fn read_tree(bytes: &[u8]) -> Result<AfNode, IoError> {
    if bytes.is_empty() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    let mut pos = 0;
    parse_node(bytes, &mut pos)
}

