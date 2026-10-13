// audience: internal
// # cn-asset-recovery-sources
// 清单内容按请求的平台加载, 实体记录保留版本与资源类别以选择归档.
// 归档成员来自中央目录, 成员缓存随目标文件的元数据变化更新.

use super::{file_stamp, is_digest, within_root, zip_reader, FileStamp};
use crate::cn_asset::{ALL_ARCHIVE_DIRECTORIES, ENTITY_MANIFESTS};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(PartialEq)]
pub(super) struct Record {
    pub name: String,
    pub digest: String,
    pub size: u64,
    pub version: String,
    pub kind: String,
}

pub(super) struct Manifest {
    pub name: &'static str,
    path: PathBuf,
    data: Option<ManifestData>,
}

struct IndexedRow {
    start: usize,
    end: usize,
    name_end: usize,
    digest_start: usize,
    digest_end: usize,
}

struct ManifestData {
    contents: String,
    rows: Vec<IndexedRow>,
    names: Vec<usize>,
    digests: Vec<usize>,
}

impl IndexedRow {
    fn name<'a>(&self, contents: &'a str) -> &'a str {
        &contents[self.start..self.name_end]
    }

    fn digest<'a>(&self, contents: &'a str) -> &'a str {
        &contents[self.digest_start..self.digest_end]
    }
}

// //// 从 CSV 行读取固定字段 [@x380kkm 2026-09-27] ////
fn fields(line: &str) -> Option<[&str; 5]> {
    let mut values = line.split(',');
    let fields = [
        values.next()?,
        values.next()?,
        values.next()?,
        values.next()?,
        values.next()?,
    ];
    values.next().is_none().then_some(fields)
}

impl ManifestData {
    // //// 用清单内偏移索引路径与摘要并保留同路径最终记录 [@x380kkm 2026-09-27] ////
    fn read(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let mut rows = Vec::new();
        let mut offset = 0;
        for raw in contents.split_inclusive('\n') {
            let trimmed = raw.trim_end_matches(&['\r', '\n'][..]);
            let line = trimmed.trim_start_matches('\u{feff}');
            let start = offset + trimmed.len() - line.len();
            offset += raw.len();
            let Some(fields) = fields(line) else { continue };
            if !fields[0].starts_with("production/")
                || !is_digest(fields[3])
                || fields[2].parse::<u64>().is_err()
                || Path::new(fields[0])
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                continue;
            }
            let digest_start = start + fields[0].len() + fields[1].len() + fields[2].len() + 3;
            rows.push(IndexedRow {
                start,
                end: start + line.len(),
                name_end: start + fields[0].len(),
                digest_start,
                digest_end: digest_start + fields[3].len(),
            });
        }
        let mut names: Vec<_> = (0..rows.len()).collect();
        names.sort_unstable_by(|left, right| {
            rows[*left]
                .name(&contents)
                .cmp(rows[*right].name(&contents))
                .then_with(|| right.cmp(left))
        });
        names.dedup_by(|left, right| rows[*left].name(&contents) == rows[*right].name(&contents));
        let mut digests = names.clone();
        digests.sort_unstable_by(|left, right| {
            rows[*left]
                .digest(&contents)
                .cmp(rows[*right].digest(&contents))
        });
        Ok(Self {
            contents,
            rows,
            names,
            digests,
        })
    }

    fn record(&self, index: usize) -> Record {
        let row = &self.rows[index];
        let fields =
            fields(&self.contents[row.start..row.end]).expect("indexed manifest row is valid");
        Record {
            name: fields[0].to_owned(),
            version: fields[1].to_owned(),
            size: fields[2].parse().expect("indexed manifest size is valid"),
            digest: fields[3].to_owned(),
            kind: fields[4].to_owned(),
        }
    }
}

impl Manifest {
    // //// 按路径或摘要读取已加载清单中的实体 [@x380kkm 2026-09-27] ////
    pub(super) fn find(
        &mut self,
        path: Option<&str>,
        digest: Option<&str>,
    ) -> io::Result<Vec<Record>> {
        if self.data.is_none() {
            self.data = Some(ManifestData::read(&self.path)?);
        }
        let data = self.data.as_ref().expect("manifest is loaded");
        if let Some(path) = path {
            return Ok(data
                .names
                .binary_search_by(|index| data.rows[*index].name(&data.contents).cmp(path))
                .ok()
                .map(|index| vec![data.record(data.names[index])])
                .unwrap_or_default());
        }
        let Some(digest) = digest else {
            return Ok(Vec::new());
        };
        let start = data
            .digests
            .partition_point(|index| data.rows[*index].digest(&data.contents) < digest);
        Ok(data.digests[start..]
            .iter()
            .take_while(|index| data.rows[**index].digest(&data.contents) == digest)
            .map(|index| data.record(*index))
            .collect())
    }
}

// //// 按覆盖目录优先级登记各平台清单 [@x380kkm 2026-09-27] ////
pub(super) fn manifests(roots: [&Path; 2]) -> Vec<Manifest> {
    let mut manifests = Vec::new();
    for directory in ["entities", "EntityLists"] {
        for name in ENTITY_MANIFESTS {
            if let Some(path) = roots
                .into_iter()
                .rev()
                .map(|root| root.join(directory).join(name))
                .find(|path| {
                    fs::metadata(path)
                        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
                })
            {
                manifests.push(Manifest {
                    name,
                    path,
                    data: None,
                });
            }
        }
    }
    manifests
}

pub(super) struct Archive {
    root: PathBuf,
    path: PathBuf,
    kind: String,
    version: String,
    fingerprint: Option<FileStamp>,
    pub members: HashSet<String>,
}

impl Archive {
    fn new(root: &Path, path: PathBuf, directory: &str, version: &str) -> Self {
        Self {
            root: root.to_owned(),
            path,
            kind: directory.split('-').nth(1).unwrap_or_default().to_owned(),
            version: version.to_owned(),
            fingerprint: None,
            members: HashSet::new(),
        }
    }

    // //// 优先选择已知成员及相同资源类别和版本的归档 [@x380kkm 2026-09-27] ////
    pub(super) fn rank(&self, records: &[Record]) -> (bool, bool, bool) {
        let known = records
            .iter()
            .any(|record| self.members.contains(&record.name));
        let (kind, version) = records
            .iter()
            .map(|record| (record.kind != self.kind, record.version != self.version))
            .min()
            .unwrap_or((true, true));
        (!known, kind, version)
    }

    // //// 校验目录边界并按目标文件状态刷新中央目录成员 [@x380kkm 2026-09-27] ////
    pub(super) fn resolve(&mut self, reader: &mut zip_reader::Cache) -> io::Result<PathBuf> {
        let path = within_root(&self.root, &self.path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "published archive is outside its resource root or missing",
            )
        })?;
        let fingerprint = file_stamp(path.clone())?;
        if self.fingerprint.as_ref() != Some(&fingerprint) {
            self.members = reader.members(&path)?;
            self.fingerprint = Some(fingerprint);
        }
        Ok(path)
    }
}

// //// 登记已发布归档的版本和类别 [@x380kkm 2026-09-27] ////
pub(super) fn archives(roots: [&Path; 2]) -> io::Result<Vec<Archive>> {
    let mut archives = Vec::new();
    for root in roots.into_iter().rev() {
        match fs::read(root.join("path")) {
            Ok(bytes) => {
                let document: serde_json::Value = serde_json::from_slice(&bytes)?;
                let groups = std::iter::once(&document["full"])
                    .chain(document["diff"].as_array().into_iter().flatten());
                for group in groups {
                    let version = group["version"].as_str().unwrap_or_default();
                    for entry in group["archive"].as_array().into_iter().flatten() {
                        let Some(location) = entry["location"].as_str() else {
                            continue;
                        };
                        let location = url::Url::parse(location)
                            .map(|url| url.path().to_owned())
                            .unwrap_or_else(|_| location.to_owned());
                        let mut parts = location.rsplit('/');
                        let (Some(name), Some(directory)) = (parts.next(), parts.next()) else {
                            continue;
                        };
                        if ALL_ARCHIVE_DIRECTORIES.contains(&directory)
                            && name.ends_with(".zip")
                            && Path::new(name)
                                .components()
                                .all(|part| matches!(part, Component::Normal(_)))
                            && Path::new(name).components().count() == 1
                        {
                            archives.push(Archive::new(
                                root,
                                root.join(directory).join(name),
                                directory,
                                version,
                            ));
                        }
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                for directory in ALL_ARCHIVE_DIRECTORIES {
                    match fs::read_dir(root.join(directory)) {
                        Ok(entries) => {
                            for entry in entries {
                                let path = entry?.path();
                                if path.extension().and_then(|value| value.to_str()) == Some("zip")
                                {
                                    archives.push(Archive::new(root, path, directory, ""));
                                }
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(archives)
}
