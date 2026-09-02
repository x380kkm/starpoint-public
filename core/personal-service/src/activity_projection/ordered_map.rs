// audience: internal
// # activity-projection-ordered-map
//
// 该模块读取和写入 CN 客户端的 zlib 索引 orderedmap. 映射顺序和 CSV 字段顺序保持不变.

use super::OrderedValue;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};

const MAX_DEPTH: usize = 64;
const MAX_ENTRIES: usize = 500_000;
const MAX_INFLATED_BYTES: usize = 128 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

// //// 解码 CN orderedmap 容器和 CSV 行 [@x380kkm 2026-08-29] ////
struct DecodeBudget {
    remaining_entries: usize,
    remaining_inflated_bytes: usize,
}

pub(super) fn decode_ordered_map(data: &[u8]) -> Result<OrderedValue, String> {
    if data.is_empty() || data.len() > MAX_INPUT_BYTES {
        return Err("CN orderedmap input size is invalid".to_owned());
    }
    let mut budget = DecodeBudget {
        remaining_entries: MAX_ENTRIES,
        remaining_inflated_bytes: MAX_INFLATED_BYTES,
    };
    decode_container(data, 1, &mut budget)
}

fn decode_container(
    data: &[u8],
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<OrderedValue, String> {
    if depth > MAX_DEPTH {
        return Err("CN orderedmap nesting depth exceeds the limit".to_owned());
    }
    let index_length = read_u32(data, 0)? as usize;
    let index_start = 4usize;
    let index_end = index_start
        .checked_add(index_length)
        .ok_or_else(|| "CN orderedmap index length overflows".to_owned())?;
    if index_length == 0 || index_end > data.len() {
        return Err("CN orderedmap index is out of bounds".to_owned());
    }
    let index = inflate_zlib(&data[index_start..index_end], budget, "index")?;
    let count = read_u32(&index, 0)? as usize;
    budget.remaining_entries = budget
        .remaining_entries
        .checked_sub(count)
        .ok_or_else(|| "CN orderedmap entry count exceeds the limit".to_owned())?;
    let table_end = 4usize
        .checked_add(
            count
                .checked_mul(8)
                .ok_or_else(|| "CN orderedmap index table overflows".to_owned())?,
        )
        .ok_or_else(|| "CN orderedmap index table overflows".to_owned())?;
    if table_end > index.len() {
        return Err("CN orderedmap index table is truncated".to_owned());
    }
    let key_bytes = &index[table_end..];
    let mut key_offset = 0usize;
    let mut data_offset = 0usize;
    let mut entries = Vec::with_capacity(count);
    for entry_index in 0..count {
        let table_offset = 4 + entry_index * 8;
        let key_end = read_u32(&index, table_offset)? as usize;
        let data_end = read_u32(&index, table_offset + 4)? as usize;
        if key_end < key_offset || key_end > key_bytes.len() {
            return Err("CN orderedmap key table is invalid".to_owned());
        }
        if data_end < data_offset {
            return Err("CN orderedmap data table is invalid".to_owned());
        }
        let data_start = index_end
            .checked_add(data_offset)
            .ok_or_else(|| "CN orderedmap data offset overflows".to_owned())?;
        let data_stop = index_end
            .checked_add(data_end)
            .ok_or_else(|| "CN orderedmap data offset overflows".to_owned())?;
        if data_stop > data.len() {
            return Err("CN orderedmap data table is invalid".to_owned());
        }
        let key = std::str::from_utf8(&key_bytes[key_offset..key_end])
            .map_err(|_| "CN orderedmap key is not UTF-8".to_owned())?
            .to_owned();
        if entries.iter().any(|(existing, _)| existing == &key) {
            return Err(format!("CN orderedmap contains duplicate key: {key}"));
        }
        let chunk = &data[data_start..data_stop];
        let value = if is_nested_container(chunk) {
            decode_container(chunk, depth + 1, budget)?
        } else {
            OrderedValue::Row(parse_csv_row(&inflate_zlib(chunk, budget, "row")?)?)
        };
        entries.push((key, value));
        key_offset = key_end;
        data_offset = data_end;
    }
    if key_offset != key_bytes.len() {
        return Err("CN orderedmap key table has trailing bytes".to_owned());
    }
    if index_end
        .checked_add(data_offset)
        .map_or(true, |end| end != data.len())
    {
        return Err("CN orderedmap value table has trailing bytes".to_owned());
    }
    Ok(OrderedValue::Map(entries))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| "CN orderedmap integer is out of bounds".to_owned())?;
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| {
        "CN orderedmap integer is out of bounds".to_owned()
    })?))
}

fn is_nested_container(data: &[u8]) -> bool {
    data.len() >= 6
        && read_u32(data, 0)
            .map(|index_length| {
                index_length > 0
                    && 4usize
                        .checked_add(index_length as usize)
                        .is_some_and(|end| end <= data.len())
            })
            .unwrap_or(false)
}

fn inflate_zlib(data: &[u8], budget: &mut DecodeBudget, section: &str) -> Result<Vec<u8>, String> {
    let mut decoder = ZlibDecoder::new(data);
    let mut inflated = Vec::new();
    decoder
        .read_to_end(&mut inflated)
        .map_err(|error| format!("CN orderedmap {section} cannot be inflated: {error}"))?;
    if decoder.total_in() != data.len() as u64 {
        return Err(format!(
            "CN orderedmap {section} has trailing compressed bytes"
        ));
    }
    budget.remaining_inflated_bytes = budget
        .remaining_inflated_bytes
        .checked_sub(inflated.len())
        .ok_or_else(|| "CN orderedmap inflated data exceeds the limit".to_owned())?;
    Ok(inflated)
}

fn parse_csv_row(data: &[u8]) -> Result<Vec<String>, String> {
    let text =
        std::str::from_utf8(data).map_err(|_| "CN orderedmap CSV row is not UTF-8".to_owned())?;
    let mut values = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut quote_closed = false;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '"' {
            if quoted && chars.peek() == Some(&'"') {
                value.push('"');
                chars.next();
            } else if quoted {
                quoted = false;
                quote_closed = true;
            } else if value.is_empty() && !quote_closed {
                quoted = true;
            } else {
                return Err("CN orderedmap CSV row has an unexpected quote".to_owned());
            }
        } else if quoted {
            value.push(character);
        } else if character == ',' {
            values.push(std::mem::take(&mut value));
            quote_closed = false;
        } else if character == '\r' || character == '\n' {
            if chars.any(|remaining| remaining != '\r' && remaining != '\n') {
                return Err("CN orderedmap CSV value contains multiple rows".to_owned());
            }
            break;
        } else {
            if quote_closed {
                return Err("CN orderedmap CSV row has data after a closing quote".to_owned());
            }
            value.push(character);
        }
    }
    if quoted {
        return Err("CN orderedmap CSV row has an unterminated quote".to_owned());
    }
    values.push(value);
    Ok(values)
}

// //// /解码 CN orderedmap 容器和 CSV 行 ////

// //// 编码 CN orderedmap 容器和 CSV 行 [@x380kkm 2026-08-29] ////
pub(super) fn encode_ordered_map(value: &OrderedValue) -> Result<Vec<u8>, String> {
    let OrderedValue::Map(entries) = value else {
        return Err("CN orderedmap root must be a map".to_owned());
    };
    encode_container(entries)
}

fn encode_container(entries: &[(String, OrderedValue)]) -> Result<Vec<u8>, String> {
    let mut keys = Vec::new();
    let mut chunks = Vec::new();
    let mut offsets = Vec::with_capacity(entries.len());
    let mut key_length = 0usize;
    let mut data_length = 0usize;
    for (key, value) in entries {
        let key_bytes = key.as_bytes();
        key_length = key_length
            .checked_add(key_bytes.len())
            .ok_or_else(|| "CN orderedmap key table overflows".to_owned())?;
        let chunk = match value {
            OrderedValue::Map(children) => encode_container(children)?,
            OrderedValue::Row(row) => zlib_encode(&encode_csv_row(row)?)?,
        };
        data_length = data_length
            .checked_add(chunk.len())
            .ok_or_else(|| "CN orderedmap data table overflows".to_owned())?;
        let key_end = u32::try_from(key_length)
            .map_err(|_| "CN orderedmap key table exceeds 32-bit size".to_owned())?;
        let data_end = u32::try_from(data_length)
            .map_err(|_| "CN orderedmap data table exceeds 32-bit size".to_owned())?;
        keys.extend_from_slice(key_bytes);
        chunks.push(chunk);
        offsets.push((key_end, data_end));
    }
    let table_length = 4usize
        .checked_add(
            offsets
                .len()
                .checked_mul(8)
                .ok_or_else(|| "CN orderedmap index table overflows".to_owned())?,
        )
        .ok_or_else(|| "CN orderedmap index table overflows".to_owned())?;
    let mut index = Vec::with_capacity(table_length + keys.len());
    push_u32(
        &mut index,
        u32::try_from(offsets.len())
            .map_err(|_| "CN orderedmap entry count exceeds 32-bit size".to_owned())?,
    );
    for (key_end, data_end) in offsets {
        push_u32(&mut index, key_end);
        push_u32(&mut index, data_end);
    }
    index.extend_from_slice(&keys);
    let compressed_index = zlib_encode(&index)?;
    let index_length = u32::try_from(compressed_index.len())
        .map_err(|_| "CN orderedmap index exceeds 32-bit size".to_owned())?;
    let mut output = Vec::with_capacity(4 + compressed_index.len() + data_length);
    push_u32(&mut output, index_length);
    output.extend_from_slice(&compressed_index);
    for chunk in chunks {
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn encode_csv_row(row: &[String]) -> Result<Vec<u8>, String> {
    let mut output = String::new();
    for (index, value) in row.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        if value
            .chars()
            .any(|character| matches!(character, ',' | '"' | '\r' | '\n'))
        {
            output.push('"');
            output.push_str(&value.replace('"', "\"\""));
            output.push('"');
        } else {
            output.push_str(value);
        }
    }
    Ok(output.into_bytes())
}

fn zlib_encode(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|error| format!("CN orderedmap zlib encoding failed: {error}"))?;
    encoder
        .finish()
        .map_err(|error| format!("CN orderedmap zlib encoding failed: {error}"))
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::{decode_ordered_map, encode_ordered_map};
    use crate::activity_projection::{projection_files::master_seed, projection_manifest};

    #[test]
    fn preserves_every_projected_activity_master() {
        for master in &projection_manifest().unwrap().masters {
            let source = master_seed(&master.name).expect("activity master seed exists");
            let value = decode_ordered_map(source).expect("activity master decodes");
            let encoded = encode_ordered_map(&value).expect("activity master encodes");
            let decoded = decode_ordered_map(&encoded).expect("encoded activity master decodes");
            assert_eq!(decoded, value);
        }
    }
}

// //// /编码 CN orderedmap 容器和 CSV 行 ////
