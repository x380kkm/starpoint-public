// audience: internal
// # activity-projection-zip-archive
//
// 该模块生成客户端差分使用的普通 ZIP 归档. 归档使用 32 位目录和未压缩 entry.

use crc32fast::Hasher as Crc32Hasher;
use std::collections::BTreeSet;

// //// 生成普通 ZIP 差分归档 [@x380kkm 2026-08-29] ////
pub(super) fn build_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if entries.len() > usize::from(u16::MAX) {
        return Err("CN activity projection ZIP has too many entries".to_owned());
    }
    let mut output = Vec::new();
    let mut central_directory = Vec::new();
    let mut names = BTreeSet::new();
    for (entry_path, data) in entries {
        if !names.insert(entry_path) {
            return Err(format!(
                "CN activity projection ZIP has duplicate entry: {entry_path}"
            ));
        }
        if entry_path.is_empty()
            || entry_path.len() > usize::from(u16::MAX)
            || !entry_path.bytes().all(|byte| byte >= 0x20 && byte != b'\\')
        {
            return Err("CN activity projection ZIP has an invalid entry path".to_owned());
        }
        let size = u32::try_from(data.len())
            .map_err(|_| "CN activity projection ZIP entry exceeds 32-bit size".to_owned())?;
        let offset = u32::try_from(output.len())
            .map_err(|_| "CN activity projection ZIP exceeds 32-bit size".to_owned())?;
        let mut hasher = Crc32Hasher::new();
        hasher.update(data);
        let crc = hasher.finalize();
        let name = entry_path.as_bytes();

        push_u32(&mut output, 0x0403_4b50);
        push_u16(&mut output, 20);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u32(&mut output, crc);
        push_u32(&mut output, size);
        push_u32(&mut output, size);
        push_u16(&mut output, name.len() as u16);
        push_u16(&mut output, 0);
        output.extend_from_slice(name);
        output.extend_from_slice(data);

        push_u32(&mut central_directory, 0x0201_4b50);
        push_u16(&mut central_directory, 20);
        push_u16(&mut central_directory, 20);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u32(&mut central_directory, crc);
        push_u32(&mut central_directory, size);
        push_u32(&mut central_directory, size);
        push_u16(&mut central_directory, name.len() as u16);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u32(&mut central_directory, 0);
        push_u32(&mut central_directory, offset);
        central_directory.extend_from_slice(name);
    }
    let central_offset = u32::try_from(output.len())
        .map_err(|_| "CN activity projection ZIP exceeds 32-bit size".to_owned())?;
    let central_size = u32::try_from(central_directory.len()).map_err(|_| {
        "CN activity projection ZIP central directory exceeds 32-bit size".to_owned()
    })?;
    output.extend_from_slice(&central_directory);
    push_u32(&mut output, 0x0605_4b50);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u16(&mut output, entries.len() as u16);
    push_u16(&mut output, entries.len() as u16);
    push_u32(&mut output, central_size);
    push_u32(&mut output, central_offset);
    push_u16(&mut output, 0);
    Ok(output)
}
// //// /生成普通 ZIP 差分归档 ////

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}
