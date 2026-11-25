use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::mem::size_of;

use anyhow::{anyhow, bail, Context, Result};

use crate::script::parser::Nls;
use crate::utils::stable_hash::StableHashMap;
use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
#[cfg(feature = "host-runtime")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "host-runtime")]
use crate::host_api::RfvpFile;

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

pub type VfsStream = Box<dyn ReadSeek + Send + Sync>;

#[derive(Clone)]
enum PackSource {
    Disk,
    #[cfg(feature = "host-runtime")]
    Host {
        file: Arc<Mutex<Box<dyn RfvpFile + Send>>>,
        len: u64,
    },
}

impl core::fmt::Debug for PackSource {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Disk => fmt.write_str("Disk"),
            #[cfg(feature = "host-runtime")]
            Self::Host { len, .. } => fmt.debug_struct("Host").field("len", len).finish(),
        }
    }
}

impl PackSource {
    fn open(&self, path: &Path) -> Result<(PackFile, u64)> {
        match self {
            Self::Disk => {
                let file = File::open(path)
                    .with_context(|| format!("open host pack {}", path.display()))?;
                let len = file.metadata()?.len();
                Ok((PackFile::Disk(file), len))
            }
            #[cfg(feature = "host-runtime")]
            Self::Host { file, len } => Ok((PackFile::Host(Arc::clone(file)), *len)),
        }
    }
}

enum PackFile {
    Disk(File),
    #[cfg(feature = "host-runtime")]
    Host(Arc<Mutex<Box<dyn RfvpFile + Send>>>),
}

/// Each resource keeps its own cursor, even when it shares a Host handle.
struct PackSlice {
    file: PackFile,
    start: u64,
    len: u64,
    pos: u64,
}

impl PackSlice {
    fn new(file: PackFile, file_len: u64, start: u64, len: u64) -> Result<Self> {
        let end = start
            .checked_add(len)
            .ok_or_else(|| anyhow!("pack slice overflow: start={start} len={len}"))?;
        if end > file_len {
            bail!("pack slice out of range: start={start} len={len} file_size={file_len}");
        }
        Ok(Self {
            file,
            start,
            len,
            pos: 0,
        })
    }

    fn clamp_pos(&self, pos: i128) -> u64 {
        if pos <= 0 {
            return 0;
        }
        let pos = pos as u128;
        if pos >= self.len as u128 {
            return self.len;
        }
        pos as u64
    }
}

impl Read for PackSlice {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let to_read = (self.len - self.pos).min(buf.len() as u64) as usize;
        if to_read == 0 {
            return Ok(0);
        }
        let offset = self.start + self.pos;
        let buf = &mut buf[..to_read];
        let read = match &mut self.file {
            PackFile::Disk(file) => {
                file.seek(SeekFrom::Start(offset))?;
                file.read(buf)?
            }
            #[cfg(feature = "host-runtime")]
            PackFile::Host(file) => file
                .lock()
                .map_err(|_| io::Error::other("Host pack file lock poisoned"))?
                .read_at(offset, buf)
                .map_err(io::Error::other)?,
        };
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated pack",
            ));
        }
        if read > to_read {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid pack read length",
            ));
        }
        self.pos += read as u64;
        Ok(read)
    }
}

impl Seek for PackSlice {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(offset) => self.clamp_pos(offset as i128),
            SeekFrom::End(delta) => self.clamp_pos(self.len as i128 + delta as i128),
            SeekFrom::Current(delta) => self.clamp_pos(self.pos as i128 + delta as i128),
        };
        self.pos = next;
        Ok(self.pos)
    }
}

#[derive(Debug, Clone)]
pub struct VfsEntry {
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct VfsFile {
    pub path: PathBuf,
    pub folder_name: String,
    pub file_count: u64,
    pub filename_table_size: u64,
    pub entries: StableHashMap<String, VfsEntry>,
    pub nls: Nls,
    source: PackSource,
    overrides: StableHashMap<String, Vec<u8>>,
}

impl VfsFile {
    pub fn from_pack_path(path: PathBuf, folder_name: String, nls: Nls) -> Result<Self> {
        Self::from_source(path, folder_name, nls, PackSource::Disk)
    }

    fn from_source(
        path: PathBuf,
        folder_name: String,
        nls: Nls,
        source: PackSource,
    ) -> Result<Self> {
        let (file, len) = source.open(&path)?;
        let mut file = PackSlice::new(file, len, 0, len)?;
        let (file_count, filename_table_size, entries) = Self::parse_reader(&mut file, nls, len)
            .with_context(|| format!("parse host pack {}", path.display()))?;
        Ok(Self {
            path,
            folder_name,
            file_count,
            filename_table_size,
            entries,
            nls,
            source,
            overrides: StableHashMap::default(),
        })
    }

    fn read_u32le(reader: &mut (impl Read + Seek), offset: u64) -> Result<u32> {
        let mut buffer = [0u8; 4];
        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut buffer)?;
        Ok(u32::from_le_bytes(buffer))
    }

    fn read_filename_table(
        reader: &mut (impl Read + Seek),
        offset: u64,
        size: u64,
        nls: Nls,
    ) -> Result<StableHashMap<u64, String>> {
        let size = usize::try_from(size).map_err(|_| anyhow!("filename table too large"))?;
        let mut buffer = Vec::new();
        buffer.resize(size, 0);
        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut buffer)?;

        let mut results = StableHashMap::default();
        let mut start = 0usize;
        for (i, &b) in buffer.iter().enumerate() {
            if b == 0 {
                let s = match nls {
                    Nls::ShiftJIS => {
                        let (s, _, _) = encoding_rs::SHIFT_JIS.decode(&buffer[start..i]);
                        s.to_string()
                    }
                    Nls::GBK => {
                        let (s, _, _) = encoding_rs::GBK.decode(&buffer[start..i]);
                        s.to_string()
                    }
                    Nls::UTF8 => String::from_utf8_lossy(&buffer[start..i]).into_owned(),
                };
                results.insert(start as u64, s);
                start = i + 1;
            }
        }
        Ok(results)
    }

    fn parse_reader(
        reader: &mut (impl Read + Seek),
        nls: Nls,
        pack_len: u64,
    ) -> Result<(u64, u64, StableHashMap<String, VfsEntry>)> {
        let mut offset = 0u64;
        let file_count = Self::read_u32le(reader, offset)? as u64;
        offset += size_of::<u32>() as u64;

        let filename_table_size = Self::read_u32le(reader, offset)? as u64;
        offset += size_of::<u32>() as u64;

        let entries_offset = offset;
        let filename_table_offset = entries_offset
            .checked_add(
                file_count
                    .checked_mul(12)
                    .ok_or_else(|| anyhow!("pack entry table overflow"))?,
            )
            .ok_or_else(|| anyhow!("pack metadata overflow"))?;
        if filename_table_offset + filename_table_size > pack_len {
            bail!("pack metadata out of range");
        }
        let filename_table =
            Self::read_filename_table(reader, filename_table_offset, filename_table_size, nls)?;

        reader.seek(SeekFrom::Start(entries_offset))?;
        let mut entries = StableHashMap::default();
        let mut cur = entries_offset;
        for _ in 0..file_count {
            let name_off = Self::read_u32le(reader, cur)? as u64;
            cur += 4;
            let data_off = Self::read_u32le(reader, cur)? as u64;
            cur += 4;
            let data_size = Self::read_u32le(reader, cur)? as u64;
            cur += 4;

            if let Some(name) = filename_table.get(&name_off) {
                entries.insert(
                    name.clone(),
                    VfsEntry {
                        offset: data_off,
                        size: data_size,
                    },
                );
            }
        }

        Ok((file_count, filename_table_size, entries))
    }

    pub fn add_override(&mut self, name: &str, bytes: Vec<u8>) {
        self.overrides.insert(name.to_string(), bytes);
    }

    pub fn open_stream_with_len(&self, name: &str) -> Result<(VfsStream, Option<u64>)> {
        if let Some(bytes) = self.overrides.get(name) {
            return Ok((
                Box::new(Cursor::new(bytes.clone())),
                Some(bytes.len() as u64),
            ));
        }

        let ent = self
            .entries
            .get(name)
            .ok_or_else(|| anyhow!("file not found in host pack {}: {}", self.folder_name, name))?;
        let (file, file_len) = self.source.open(&self.path)?;
        let sub_file = PackSlice::new(file, file_len, ent.offset, ent.size)
            .with_context(|| format!("open pack entry {}/{}", self.folder_name, name))?;
        Ok((Box::new(sub_file), Some(ent.size)))
    }

    pub fn open_stream(&self, name: &str) -> Result<VfsStream> {
        self.open_stream_with_len(name).map(|(stream, _)| stream)
    }

    pub fn read_file(&self, name: &str) -> Result<Vec<u8>> {
        let (mut stream, len) = self.open_stream_with_len(name)?;
        let mut out = Vec::with_capacity(len.and_then(|n| usize::try_from(n).ok()).unwrap_or(0));
        stream.read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn save(&mut self, name: &str, content: Vec<u8>) -> Result<()> {
        self.overrides.insert(name.to_string(), content);
        Ok(())
    }
}

#[derive(Debug)]
pub struct Vfs {
    pub files: StableHashMap<String, VfsFile>,
    loose_files: StableHashMap<String, Vec<u8>>,
    pub nls: Nls,
}

impl Default for Vfs {
    fn default() -> Self {
        Self {
            files: StableHashMap::default(),
            loose_files: StableHashMap::default(),
            nls: Nls::ShiftJIS,
        }
    }
}

impl Vfs {
    pub fn new(nls: Nls) -> Result<Self> {
        Ok(Self {
            nls,
            ..Self::default()
        })
    }

    pub fn add_loose_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.loose_files.insert(normalize_vfs_key(path), bytes);
    }

    pub fn add_pack_file(&mut self, path: PathBuf, folder_name: &str) -> Result<()> {
        self.add_pack(path, folder_name, PackSource::Disk)
    }

    #[cfg(feature = "host-runtime")]
    pub(crate) fn add_host_pack(
        &mut self,
        path: &str,
        folder_name: &str,
        mut file: impl RfvpFile + Send + 'static,
    ) -> Result<()> {
        let len = file.len().map_err(io::Error::other)?;
        let source = PackSource::Host {
            file: Arc::new(Mutex::new(Box::new(file))),
            len,
        };
        self.add_pack(PathBuf::from(path), folder_name, source)
    }

    fn add_pack(&mut self, path: PathBuf, folder_name: &str, source: PackSource) -> Result<()> {
        let normalized = folder_name.to_ascii_lowercase();
        let folder = normalized.strip_suffix(".bin").unwrap_or(&normalized);
        let file = VfsFile::from_source(path, folder.to_string(), self.nls, source)?;
        self.files.insert(folder.to_string(), file);
        Ok(())
    }

    pub fn open_stream_with_len(&self, path: &str) -> Result<(VfsStream, Option<u64>)> {
        let key = normalize_vfs_key(path);
        if let Some(bytes) = self.loose_files.get(&key) {
            return Ok((
                Box::new(Cursor::new(bytes.clone())),
                Some(bytes.len() as u64),
            ));
        }

        let normalized = path.replace('\\', "/");
        let normalized = normalized.trim_start_matches("./").trim_start_matches('/');
        let (folder, inner) = normalized
            .split_once('/')
            .ok_or_else(|| anyhow!("file not found: {}", path))?;
        let file = self
            .files
            .get(&folder.to_ascii_lowercase())
            .ok_or_else(|| anyhow!("pack not found for folder '{}'", folder))?;
        file.open_stream_with_len(inner)
    }

    pub fn open_stream(&self, path: &str) -> Result<VfsStream> {
        self.open_stream_with_len(path).map(|(stream, _)| stream)
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let (mut stream, len) = self.open_stream_with_len(path)?;
        let mut out = Vec::with_capacity(len.and_then(|n| usize::try_from(n).ok()).unwrap_or(0));
        stream.read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn save(&mut self, path: &str, content: Vec<u8>) -> Result<()> {
        let key = normalize_vfs_key(path);
        let normalized = path.replace('\\', "/");
        let normalized = normalized.trim_start_matches("./").trim_start_matches('/');
        if let Some((folder, inner)) = normalized.split_once('/') {
            if let Some(file) = self.files.get_mut(&folder.to_ascii_lowercase()) {
                file.save(inner, content)?;
                return Ok(());
            }
        }
        self.loose_files.insert(key, content);
        Ok(())
    }

    pub fn find_ani(&self) -> Result<Vec<PathBuf>> {
        Ok(self
            .loose_files
            .keys()
            .filter(|key| {
                let lower = key.to_ascii_lowercase();
                lower.starts_with("cursor") && lower.ends_with(".ani")
            })
            .map(|key| PathBuf::from(key.as_str()))
            .collect())
    }
}

fn normalize_vfs_key(path: &str) -> String {
    path.trim_start_matches("./")
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

#[cfg(all(test, feature = "host-runtime"))]
mod tests {
    use super::*;
    use crate::host_api::RfvpResult;

    struct ShortReads(Vec<u8>);

    impl RfvpFile for ShortReads {
        fn len(&mut self) -> RfvpResult<u64> {
            Ok(self.0.len() as u64)
        }

        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RfvpResult<usize> {
            let tail = self.0.get(offset as usize..).unwrap_or_default();
            let len = tail.len().min(buf.len()).min(2);
            buf[..len].copy_from_slice(&tail[..len]);
            Ok(len)
        }
    }

    #[test]
    fn host_pack_reads_short_and_keeps_independent_entry_cursors() -> Result<()> {
        let names = b"one\0two\0bad\0";
        let payload_start = 8 + 3 * 12 + names.len() as u32;
        let mut pack = Vec::new();
        for field in [
            3,
            names.len() as u32,
            0,
            payload_start,
            4,
            4,
            payload_start + 4,
            3,
            8,
            payload_start + 7,
            1,
        ] {
            pack.extend_from_slice(&field.to_le_bytes());
        }
        pack.extend_from_slice(names);
        pack.extend_from_slice(b"abcdXYZ");

        let mut vfs = Vfs::new(Nls::UTF8)?;
        vfs.add_host_pack("memory:/data.BIN", "data.BIN", ShortReads(pack))?;
        let (mut first, len) = vfs.open_stream_with_len("data/one")?;
        let mut second = vfs.open_stream("data/two")?;
        assert_eq!(len, Some(4));
        assert!(vfs.open_stream("data/bad").is_err());
        assert!(vfs.open_stream("data/missing").is_err());
        drop(vfs);

        assert_eq!(first.seek(SeekFrom::Start(1))?, 1);
        let mut actual = Vec::new();
        second.read_to_end(&mut actual)?;
        assert_eq!(actual, b"XYZ");
        actual.clear();
        first.read_to_end(&mut actual)?;
        assert_eq!(actual, b"bcd");
        assert_eq!(first.seek(SeekFrom::End(-10))?, 0);
        assert_eq!(first.seek(SeekFrom::Start(10))?, 4);
        Ok(())
    }
}
