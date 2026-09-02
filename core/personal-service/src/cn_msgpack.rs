// audience: internal
// # cn-msgpack
//
// 该模块将 CN 响应中的 MessagePack 数字转换为旧客户端接受的标签.

use crate::PersonalServiceError;

// //// 转换 MessagePack uint32 标签 [@x380kkm 2026-08-24] ////
pub(crate) fn normalize_client_msgpack_numbers(
    packed: &[u8],
) -> Result<Vec<u8>, PersonalServiceError> {
    let mut normalized = Vec::with_capacity(packed.len());
    let mut offset = 0;
    while offset < packed.len() {
        offset = copy_msgpack_value(packed, offset, &mut normalized)?;
    }
    Ok(normalized)
}

fn copy_msgpack_value(
    packed: &[u8],
    offset: usize,
    normalized: &mut Vec<u8>,
) -> Result<usize, PersonalServiceError> {
    let marker = *packed.get(offset).ok_or_else(invalid_msgpack_response)?;
    if marker <= 0x7f || marker >= 0xe0 || matches!(marker, 0xc0 | 0xc2 | 0xc3) {
        normalized.push(marker);
        return Ok(offset + 1);
    }

    match marker {
        0xcc | 0xd0 => copy_msgpack_bytes(packed, offset, 2, normalized),
        0xcd | 0xd1 => copy_msgpack_bytes(packed, offset, 3, normalized),
        0xce => {
            let encoded = packed
                .get(offset + 1..offset + 5)
                .ok_or_else(invalid_msgpack_response)?;
            let value = u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
            if value < 0x8000_0000 {
                normalized.push(0xd2);
                normalized.extend_from_slice(encoded);
            } else {
                normalized.push(0xcb);
                normalized.extend_from_slice(&f64::from(value).to_be_bytes());
            }
            Ok(offset + 5)
        }
        0xca | 0xd2 => copy_msgpack_bytes(packed, offset, 5, normalized),
        0xcb | 0xcf | 0xd3 => copy_msgpack_bytes(packed, offset, 9, normalized),
        0xc4 | 0xd9 => copy_length_prefixed_msgpack_bytes(packed, offset, 1, normalized),
        0xc5 | 0xda => copy_length_prefixed_msgpack_bytes(packed, offset, 2, normalized),
        0xc6 | 0xdb => copy_length_prefixed_msgpack_bytes(packed, offset, 4, normalized),
        0xc7 => copy_ext_msgpack_bytes(packed, offset, 1, normalized),
        0xc8 => copy_ext_msgpack_bytes(packed, offset, 2, normalized),
        0xc9 => copy_ext_msgpack_bytes(packed, offset, 4, normalized),
        0xd4 => copy_msgpack_bytes(packed, offset, 3, normalized),
        0xd5 => copy_msgpack_bytes(packed, offset, 4, normalized),
        0xd6 => copy_msgpack_bytes(packed, offset, 6, normalized),
        0xd7 => copy_msgpack_bytes(packed, offset, 10, normalized),
        0xd8 => copy_msgpack_bytes(packed, offset, 18, normalized),
        0xdc => copy_msgpack_array(packed, offset, 2, normalized),
        0xdd => copy_msgpack_array(packed, offset, 4, normalized),
        0xde => copy_msgpack_map(packed, offset, 2, normalized),
        0xdf => copy_msgpack_map(packed, offset, 4, normalized),
        0xa0..=0xbf => {
            copy_msgpack_bytes(packed, offset, 1 + usize::from(marker & 0x1f), normalized)
        }
        0x90..=0x9f => {
            normalized.push(marker);
            copy_msgpack_values(packed, offset + 1, usize::from(marker & 0x0f), normalized)
        }
        0x80..=0x8f => {
            normalized.push(marker);
            copy_msgpack_values(
                packed,
                offset + 1,
                usize::from(marker & 0x0f) * 2,
                normalized,
            )
        }
        _ => Err(invalid_msgpack_response()),
    }
}

fn copy_length_prefixed_msgpack_bytes(
    packed: &[u8],
    offset: usize,
    length_bytes: usize,
    normalized: &mut Vec<u8>,
) -> Result<usize, PersonalServiceError> {
    let content_length = read_msgpack_length(packed, offset + 1, length_bytes)?;
    copy_msgpack_bytes(
        packed,
        offset,
        1 + length_bytes + content_length,
        normalized,
    )
}

fn copy_ext_msgpack_bytes(
    packed: &[u8],
    offset: usize,
    length_bytes: usize,
    normalized: &mut Vec<u8>,
) -> Result<usize, PersonalServiceError> {
    let content_length = read_msgpack_length(packed, offset + 1, length_bytes)?;
    copy_msgpack_bytes(
        packed,
        offset,
        2 + length_bytes + content_length,
        normalized,
    )
}

fn copy_msgpack_array(
    packed: &[u8],
    offset: usize,
    length_bytes: usize,
    normalized: &mut Vec<u8>,
) -> Result<usize, PersonalServiceError> {
    let count = read_msgpack_length(packed, offset + 1, length_bytes)?;
    let values_offset = copy_msgpack_bytes(packed, offset, 1 + length_bytes, normalized)?;
    copy_msgpack_values(packed, values_offset, count, normalized)
}

fn copy_msgpack_map(
    packed: &[u8],
    offset: usize,
    length_bytes: usize,
    normalized: &mut Vec<u8>,
) -> Result<usize, PersonalServiceError> {
    let count = read_msgpack_length(packed, offset + 1, length_bytes)?;
    let values_offset = copy_msgpack_bytes(packed, offset, 1 + length_bytes, normalized)?;
    let value_count = count.checked_mul(2).ok_or_else(invalid_msgpack_response)?;
    copy_msgpack_values(packed, values_offset, value_count, normalized)
}

fn copy_msgpack_values(
    packed: &[u8],
    mut offset: usize,
    count: usize,
    normalized: &mut Vec<u8>,
) -> Result<usize, PersonalServiceError> {
    for _ in 0..count {
        offset = copy_msgpack_value(packed, offset, normalized)?;
    }
    Ok(offset)
}

fn copy_msgpack_bytes(
    packed: &[u8],
    offset: usize,
    length: usize,
    normalized: &mut Vec<u8>,
) -> Result<usize, PersonalServiceError> {
    let end = offset
        .checked_add(length)
        .ok_or_else(invalid_msgpack_response)?;
    let source = packed
        .get(offset..end)
        .ok_or_else(invalid_msgpack_response)?;
    normalized.extend_from_slice(source);
    Ok(end)
}

fn read_msgpack_length(
    packed: &[u8],
    offset: usize,
    length_bytes: usize,
) -> Result<usize, PersonalServiceError> {
    let end = offset
        .checked_add(length_bytes)
        .ok_or_else(invalid_msgpack_response)?;
    let encoded = packed
        .get(offset..end)
        .ok_or_else(invalid_msgpack_response)?;
    let length = match encoded {
        [value] => u32::from(*value),
        [high, low] => u32::from(u16::from_be_bytes([*high, *low])),
        [a, b, c, d] => u32::from_be_bytes([*a, *b, *c, *d]),
        _ => return Err(invalid_msgpack_response()),
    };
    usize::try_from(length).map_err(|_| invalid_msgpack_response())
}

fn invalid_msgpack_response() -> PersonalServiceError {
    PersonalServiceError::new("failed to normalize CN MessagePack response")
}
// //// /转换 MessagePack uint32 标签 ////

// //// 验证 MessagePack 数字标签转换 [@x380kkm 2026-08-24] ////
#[cfg(test)]
mod tests {
    use super::normalize_client_msgpack_numbers;

    #[test]
    fn normalizes_uint32_tags_outside_binary_values() {
        let packed = vec![
            0x93, 0xce, 0x7f, 0xff, 0xff, 0xff, 0xce, 0x80, 0x00, 0x00, 0x00, 0xc4, 0x05, 0xce,
            0x00, 0x00, 0x00, 0x01,
        ];
        let mut expected = vec![0x93, 0xd2, 0x7f, 0xff, 0xff, 0xff, 0xcb];
        expected.extend_from_slice(&2_147_483_648_f64.to_be_bytes());
        expected.extend_from_slice(&[0xc4, 0x05, 0xce, 0x00, 0x00, 0x00, 0x01]);

        assert_eq!(
            normalize_client_msgpack_numbers(&packed).expect("response normalizes"),
            expected
        );
    }
}
// //// /验证 MessagePack 数字标签转换 ////
