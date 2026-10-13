// audience: internal
// # cn-asset-recovery-zip-reader
// ZIP 解码器通过实体名称和长度返回单个文件, 所有内容保持在内存中.
// 最近使用的归档复用中央目录; 每次读取按文件长度与修改时间识别原位替换.

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_CACHED_ARCHIVES: usize = 8;

#[derive(Default)]
pub(super) struct Cache {
    archives: VecDeque<CachedArchive>,
}

struct CachedArchive {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    archive: zip::ZipArchive<BufReader<File>>,
}

impl Cache {
    // //// 从中央目录读取成员名并保留原始文件内容 [@x380kkm 2026-09-27] ////
    pub(super) fn members(&mut self, path: &Path) -> io::Result<HashSet<String>> {
        Ok(self
            .archive(path)?
            .file_names()
            .filter(|name| !name.ends_with('/'))
            .map(str::to_owned)
            .collect())
    }

    // //// 按清单长度读取指定归档成员 [@x380kkm 2026-09-27] ////
    pub(super) fn read(&mut self, path: &Path, name: &str, size: u64) -> io::Result<Vec<u8>> {
        read(self.archive(path)?, name, size)
    }

    // //// 按目标归档状态复用中央目录并限制打开文件数量 [@x380kkm 2026-09-27] ////
    fn archive(&mut self, path: &Path) -> io::Result<&mut zip::ZipArchive<BufReader<File>>> {
        let metadata = path.metadata()?;
        let modified = metadata.modified().ok();
        let previous = self
            .archives
            .iter()
            .position(|entry| entry.path == path)
            .and_then(|index| self.archives.remove(index));
        let archive = match previous {
            Some(archive) if archive.size == metadata.len() && archive.modified == modified => {
                archive
            }
            _ => CachedArchive {
                path: path.to_owned(),
                size: metadata.len(),
                modified,
                archive: zip::ZipArchive::new(BufReader::new(File::open(path)?))?,
            },
        };
        self.archives.push_front(archive);
        self.archives.truncate(MAX_CACHED_ARCHIVES);
        Ok(&mut self
            .archives
            .front_mut()
            .expect("archive is cached")
            .archive)
    }

    // //// 释放归档句柄与中央目录 [@x380kkm 2026-09-27] ////
    pub(super) fn clear(&mut self) {
        self.archives.clear();
    }
}

// //// 按清单长度解压单个实体并保留 ZIP 校验 [@x380kkm 2026-09-20] ////
fn read(
    archive: &mut zip::ZipArchive<BufReader<File>>,
    name: &str,
    size: u64,
) -> io::Result<Vec<u8>> {
    let entry = archive.by_name(name)?;
    if entry.size() != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ZIP entity length differs from manifest",
        ));
    }
    let mut body = Vec::new();
    entry.take(size.saturating_add(1)).read_to_end(&mut body)?;
    Ok(body)
}
