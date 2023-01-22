use std::path::Path;

use std::fs::File;
use std::io::prelude::*;

use std::path::PathBuf;

use anyhow::{bail, Result};
use std::collections::HashMap;

use lexiclean::Lexiclean;
use lzma_rs::lzma_decompress;
use memmap::MmapOptions;

use byteorder_pack::UnpackFrom;

const JFFS2_NODETYPE_DIRENT: u16 = 0xE001;
const JFFS2_NODETYPE_INODE: u16 = 0xE002;

const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

const JFFS2_COMPR_NONE: u8 = 0x00;
const JFFS2_COMPR_ZERO: u8 = 0x01;
const JFFS2_COMPR_RTIME: u8 = 0x02;
const JFFS2_COMPR_RUBINMIPS: u8 = 0x03;
const JFFS2_COMPR_COPY: u8 = 0x04;
const JFFS2_COMPR_DYNRUBIN: u8 = 0x05;
const JFFS2_COMPR_ZLIB: u8 = 0x06;
const JFFS2_COMPR_LZO: u8 = 0x07;
const JFFS2_COMPR_LZMA: u8 = 0x08;

const SIZE_OF_DIRENT: usize = 28;
const SIZE_OF_INODE: usize = 56;

const LZMA_BEST_LC: u8 = 0;
const LZMA_BEST_LP: u8 = 0;
const LZMA_BEST_PB: u8 = 0;

const DICT_SIZE: u32 = 0x2000;

use std::os::raw::{c_int, c_uchar, c_uint, c_void};
use std::path::Component;

extern "C" {

    fn dynrubin_decompress(
        data_in: *const c_uchar,
        cpage_out: *const c_uchar,
        sourcelen: c_uint,
        dstlen: c_uint,
    ) -> c_void;

    fn lzo1x_decompress_safe(
        in_data: *const c_uchar,
        in_len: usize,
        out: *const c_uchar,
        out_len: *const usize,
        wrkmem: *const c_void,
    ) -> c_int;
}

pub trait JffsPathFixer {
    fn jffs_fix(self) -> PathBuf;
}

impl JffsPathFixer for &Path {
    fn jffs_fix(self) -> PathBuf {
        if self.components().count() <= 1 {
            return self.to_owned();
        }

        let mut components = Vec::new();
        let last = &self.components().last();
        let mut ignore_last = false;
        if let Some(Component::Normal(a)) = last {
            if a.is_empty() {
                ignore_last = true;
            }
        }

        for component in self.components() {
            components.push(component);
        }

        if ignore_last {
            components.pop();
        }

        components.into_iter().collect()
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Jffs2Dirent {
    // jint32_t pino;
    // jint32_t version;
    // jint32_t ino; /* == zero for unlink */
    // jint32_t mctime;
    // uint8_t nsize;
    // uint8_t type;
    // uint8_t unused[2];
    // jint32_t node_crc;
    // jint32_t name_crc;
    // uint8_t name[0];
    pino: u32,
    version: u32,
    mctime: u32,
    ntype: u8,
    fname: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Jffs2Inode {
    // jint32_t ino;        /* Inode number.  */
    // jint32_t version;    /* Version number.  */
    // jmode_t mode;       /* The file's type or mode.  */
    // jint16_t uid;        /* The file's owner.  */
    // jint16_t gid;        /* The file's group.  */
    // jint32_t isize;      /* Total resultant size of this inode (used for truncations)  */
    // jint32_t atime;      /* Last access time.  */
    // jint32_t mtime;      /* Last modification time.  */
    // jint32_t ctime;      /* Change time.  */
    // jint32_t offset;     /* Where to begin to write.  */
    // jint32_t csize;      /* (Compressed) data size */
    // jint32_t dsize;      /* Size of the node's data. (after decompression) */
    // uint8_t compr;       /* Compression algorithm used */
    // uint8_t usercompr;   /* Compression algorithm requested by the user */
    // jint16_t flags;      /* See JFFS2_INO_FLAG_* */
    // jint32_t data_crc;   /* CRC for the (compressed) data.  */
    // jint32_t node_crc;   /* CRC for the raw inode (excluding data)  */
    // uint8_t data[0];
    version: u32,
    iszie: u32,
    mtime: u32,
    offset: u32,
    csize: u32,
    dsize: u32,
    compr: u8,
    data: u32,
}

impl Jffs2Inode {
    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Size after compression
    pub fn compressed_size(&self) -> u32 {
        self.csize
    }
    
    /// Original size
    pub fn decompressed_size(&self) -> u32 {
        self.dsize
    }

    /// Compression method
    pub fn compression_method(&self) -> u8 {
        self.compr
    }

    /// Data Offset in the file
    pub fn data_offset(&self) -> u32 {
        self.data
    }
}

#[derive(Debug, Clone)]
pub struct Jffs2Entry {
    inodes: Vec<Jffs2Inode>,
    is_file: bool,
    path: PathBuf,
}

impl Jffs2Entry {
    /// The original file size of the dirent
    pub fn size(&self) -> u64 {
        let mut dirent_size = 0 as u64;
        for node in &self.inodes {
            dirent_size += node.decompressed_size() as u64;
        }
        
        dirent_size
    }

    /// Returns true if the current dirent represents a file, 
    /// otherwise, the current dirent represents a folder
    pub fn is_file(&self) -> bool {
        self.is_file
    }

    /// Path of the current dirent within the filesystem
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[derive(Debug)]
struct Jffs2Reader {
    buffer: memmap::Mmap,
    little_endian: bool,
    dirents: HashMap<u32, Jffs2Dirent>,
    inodes: HashMap<u32, Vec<Jffs2Inode>>,
}

// reference :
// https://github.com/sviehb/jefferson/blob/master/src/scripts/jefferson

impl Jffs2Reader {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        let buffer = unsafe { MmapOptions::new().map(&file)? };
        if buffer.len() < 2 {
            bail!("image size is too small");
        }

        let initial = Jffs2Reader::read_uint16(&buffer[0..2], true, 0)?;
        if initial != 0x1985 && initial != 0x8519 {
            bail!("image is not jffs2");
        }

        let little_endian = initial == 0x1985;
        Ok(Jffs2Reader {
            buffer,
            little_endian,
            dirents: HashMap::new(),
            inodes: HashMap::new(),
        })
    }

    fn read_uint32(buffer: &[u8], little_endian: bool, offset: usize) -> Result<u32> {
        if offset + 4 > buffer.len() {
            bail!(
                "offset out of bounds: {} in a buffer of {}",
                offset,
                buffer.len()
            );
        }
        let buffer = &buffer[offset..offset + 4];

        Ok(if little_endian {
            u32::from_le_bytes(buffer.try_into().unwrap())
        } else {
            u32::from_be_bytes(buffer.try_into().unwrap())
        })
    }

    fn read_uint16(buffer: &[u8], little_endian: bool, offset: usize) -> Result<u16> {
        if offset + 2 > buffer.len() {
            bail!(
                "offset out of bounds: {} in a buffer of {}",
                offset,
                buffer.len()
            );
        }
        let buffer = &buffer[offset..offset + 2];

        Ok(if little_endian {
            u16::from_le_bytes(buffer.try_into().unwrap())
        } else {
            u16::from_be_bytes(buffer.try_into().unwrap())
        })
    }

    /// Read a string with at most `length` bytes, but will truncate before
    /// that if there is a null byte.
    fn read_str(buffer: &[u8], offset: usize, length: usize) -> Result<String> {
        if offset >= buffer.len() {
            bail!(
                "offset out of bounds: {} in a buffer of {}",
                offset,
                buffer.len()
            );
        }

        let str_bytes = buffer
            .iter()
            .skip(offset)
            .take(length)
            .take_while(|b| **b != 0)
            .copied()
            .collect();

        let s = String::from_utf8(str_bytes)?;
        Ok(s)
    }

    fn scan_dirent(&mut self, mm: &[u8]) -> Result<bool> {
        if mm.len() < SIZE_OF_DIRENT {
            return Ok(false);
        }

        let mut cur = std::io::Cursor::new(mm);

        let (pino, version, ino, mctime) = <(u32, u32, u32, u32)>::unpack_from_le(&mut cur)?;
        let (nsize, ntype) = <(u8, u8)>::unpack_from_le(&mut cur)?;
        let (_unused, _node_crc, _name_crc) = <(u16, u32, u32)>::unpack_from_le(&mut cur)?;

        if nsize as usize + SIZE_OF_DIRENT > mm.len() {
            bail!("out of bounds when reading filename");
        }

        if let Some(old_dirent) = self.dirents.get(&ino) {
            if old_dirent.version > version {
                return Ok(true);
            }
        }

        let fname = Jffs2Reader::read_str(mm, cur.position() as usize, nsize as usize)?;
        self.dirents.insert(
            ino,
            Jffs2Dirent {
                pino,
                version,
                mctime,
                ntype,
                fname,
            },
        );

        Ok(true)
    }

    fn scan_inode(&mut self, mm: &[u8], idx: u32) -> Result<bool> {
        if mm.len() < SIZE_OF_INODE {
            return Ok(false);
        }

        let mut cur = std::io::Cursor::new(mm);

        let (ino, version, _mode, _uid, _gid) =
            <(u32, u32, u32, u16, u16)>::unpack_from_le(&mut cur)?;
        let (isize, _atime, mtime, _ctime) = <(u32, u32, u32, u32)>::unpack_from_le(&mut cur)?;
        let (foffset, csize, dsize, compr, _usercompr) =
            <(u32, u32, u32, u8, u8)>::unpack_from_le(&mut cur)?;
        let (_flags, _data_crc, _node_crc) = <(u16, u32, u32)>::unpack_from_le(&mut cur)?;

        if csize as usize + SIZE_OF_INODE > mm.len() {
            bail!("out of bounds when reading data");
        }

        if let Some(inodes) = self.inodes.get(&ino) {
            for old_inode in inodes {
                if old_inode.version > version && foffset == old_inode.offset {
                    return Ok(true);
                }
            }
        }

        let data = idx + SIZE_OF_INODE as u32;
        let new_node = Jffs2Inode {
            version,
            iszie: isize,
            mtime,
            offset: foffset,
            csize,
            dsize,
            compr,
            data,
        };

        match self.inodes.get_mut(&ino) {
            Some(inodes) => {
                inodes.push(new_node);
            }
            _ => {
                let inodes = vec![new_node];
                self.inodes.insert(ino, inodes);
            }
        }

        Ok(true)
    }

    fn pad(x: u32) -> u32 {
        if x % 4 != 0 {
            x + (4 - (x % 4))
        } else {
            x
        }
    }

    pub fn scan(&mut self) -> Result<()> {
        let mut idx = 0;
        let maxmm = self.buffer.len() as u32;

        while idx < maxmm - 12 {
            let magic = Jffs2Reader::read_uint16(&self.buffer, self.little_endian, idx as usize)?;
            if magic != 0x1985 {
                // plus 4 here, rather than 2
                idx += 4;
                continue;
            }

            idx += 2;

            let nodetype =
                Jffs2Reader::read_uint16(&self.buffer, self.little_endian, idx as usize)?;
            idx += 2;

            let totlen = Jffs2Reader::read_uint32(&self.buffer, self.little_endian, idx as usize)?;
            idx += 4;

            let _hdh_crc =
                Jffs2Reader::read_uint32(&self.buffer, self.little_endian, idx as usize)?;
            idx += 4;

            if totlen > maxmm - idx || totlen == 0 {
                break;
            }

            if nodetype == JFFS2_NODETYPE_DIRENT {
                idx -= 12;
                let slice =
                    self.buffer[idx as usize + 12..idx as usize + totlen as usize].to_owned();
                self.scan_dirent(&slice)?;
            } else if nodetype == JFFS2_NODETYPE_INODE {
                idx -= 12;
                let slice =
                    self.buffer[idx as usize + 12..idx as usize + totlen as usize].to_owned();
                self.scan_inode(&slice, idx + 12)?;
            }

            idx += Jffs2Reader::pad(totlen);
        }

        Ok(())
    }

    fn rtime_decompress(compressed_buffer: &[u8], dstlen: usize) -> Vec<u8> {
        let mut dst = vec![];
        let mut pos = 0;
        let mut position = Vec::new();
        position.resize(256, 0);

        while dst.len() < dstlen {
            let val = &compressed_buffer[pos..pos + 1];
            pos += 1;
            let val = val[0];
            dst.push(val);

            let repeat = &compressed_buffer[pos..pos + 1];
            let mut repeat = repeat[0];
            pos += 1;
            let mut backoffs = position[val as usize];

            position[val as usize] = dst.len();
            if repeat != 0 {
                if backoffs + repeat as usize >= dst.len() {
                    while repeat != 0 {
                        dst.push(dst[backoffs]);
                        backoffs += 1;
                        repeat -= 1;
                    }
                } else {
                    let slice = &dst[backoffs..backoffs + repeat as usize].to_owned();
                    dst.extend(slice);
                }
            }
        }

        dst
    }

    fn dump_file(&self, output_path: &PathBuf, node: u32) -> Result<()> {
        let inodes = match self.inodes.get(&node) {
            Some(inodes) => inodes,
            None => return Ok(()),
        };

        let mut sorted_inodes = inodes.clone();
        sorted_inodes.sort_by_key(|k| k.offset);
        if let Some(dirname) = output_path.parent() {
            if !dirname.exists() {
                std::fs::create_dir_all(dirname)?;
            }
        }
        let mut file = File::create(output_path.jffs_fix())?;
        for inode in sorted_inodes {
            if inode.compr == JFFS2_COMPR_NONE {
                file.write_all(
                    &self.buffer[inode.data as usize..(inode.data + inode.csize) as usize],
                )?;
            } else if inode.compr == JFFS2_COMPR_ZERO {
                let cycle = inode.dsize / 0x1000;
                let reminder = inode.dsize % 0x1000;
                for _ in 0..cycle {
                    file.write_all(&vec![0; 0x1000])?;
                }
                if reminder != 0 {
                    file.write_all(&vec![0; reminder as usize])?;
                }
            } else if inode.compr == JFFS2_COMPR_ZLIB {
                let mut decomp = flate2::read::ZlibDecoder::new(
                    &self.buffer[inode.data as usize..(inode.data + inode.csize) as usize],
                );
                let mut buf = Vec::new();
                decomp.read_to_end(&mut buf)?;
                file.write_all(&buf)?;
            } else if inode.compr == JFFS2_COMPR_RTIME {
                let buf = Jffs2Reader::rtime_decompress(
                    &self.buffer[inode.data as usize..(inode.data + inode.csize) as usize],
                    inode.dsize as usize,
                );

                file.write_all(&buf)?;
            } else if inode.compr == JFFS2_COMPR_LZO {
                let mut decomp: Vec<u8> = Vec::new();
                let decompressed_size = inode.dsize as usize;
                decomp.resize(inode.dsize as usize, 0);

                let input = &self.buffer[inode.data as usize..(inode.data + inode.csize) as usize];

                unsafe {
                    lzo1x_decompress_safe(
                        input.as_ptr(),
                        input.len(),
                        decomp.as_mut_ptr(),
                        &decompressed_size,
                        std::ptr::null(),
                    );
                }

                file.write_all(&decomp)?;
            } else if inode.compr == JFFS2_COMPR_LZMA {
                let pb = LZMA_BEST_PB;
                let lp = LZMA_BEST_LP;
                let lc = LZMA_BEST_LC;

                // reconstruct the lzma header
                // lzma_header = struct.pack("<BIQ", PROPERTIES, DICT_SIZE, outlen)
                let mut input: Vec<u8> = Vec::new();

                let properties = (pb * 5 + lp) * 9 + lc;
                input.push(properties);

                let dict_size = DICT_SIZE.to_le_bytes();
                input.extend(dict_size);

                let out_len = (inode.dsize as u64).to_le_bytes();
                input.extend(out_len);

                // append the compressed blob
                input
                    .extend(&self.buffer[inode.data as usize..(inode.data + inode.csize) as usize]);

                let mut decomp: Vec<u8> = Vec::new();
                let mut input_reader = std::io::Cursor::new(&input);
                lzma_decompress(&mut input_reader, &mut decomp)?;

                file.write_all(&decomp)?;
            } else if inode.compr == JFFS2_COMPR_DYNRUBIN {
                // this is slow but it works
                let mut decomp: Vec<u8> = Vec::new();
                decomp.resize(inode.dsize as usize, 0);
                let input = &self.buffer[inode.data as usize..(inode.data + inode.csize) as usize];

                unsafe {
                    dynrubin_decompress(
                        input.as_ptr() as *const u8,
                        decomp.as_mut_ptr() as *mut u8,
                        input.len() as c_uint,
                        inode.dsize as u32,
                    );
                }

                file.write_all(&decomp)?;
            } else if inode.compr == JFFS2_COMPR_RUBINMIPS {
                bail!("JFFS2_COMPR_RUBINMIPS is deprecated!!");
            } else if inode.compr == JFFS2_COMPR_COPY {
                bail!("JFFS2_COMPR_COPY is never implemented!");
            } else {
                bail!("unknown compression type");
            }
        }

        Ok(())
    }

    fn resolve_dirent(&self, node: u32) -> Result<(PathBuf, u8)> {
        let mut path = PathBuf::new();
        let (ntype, mut cnode) = match self.dirents.get(&node) {
            Some(dirent) => (dirent.ntype, dirent.clone()),
            _ => bail!("no dirent for node {}", node),
        };

        for _i in 0..32 {
            if cnode.pino == 1 {
                let fname = cnode.fname;
                let name_path = Path::new(&fname);
                let mut output_path = name_path.join(path);
                output_path = output_path.lexiclean().jffs_fix();
                return Ok((output_path, ntype));
            } else {
                let name_path = Path::new(&cnode.fname);
                path = name_path.join(path);
                cnode = match self.dirents.get(&cnode.pino) {
                    Some(dirent) => dirent.clone(),
                    _ => bail!("cannot find parent node {}", cnode.pino),
                };
            }
        }

        bail!("cannot resolve dirent {}", node);
    }

    pub fn dump(&self, target_path: impl AsRef<Path>) -> Result<()> {
        for i in self.dirents.keys() {
            let (output_path, ntype) = self.resolve_dirent(*i)?;
            if ntype == DT_DIR {
                std::fs::create_dir_all(target_path.as_ref().join(output_path))?;
            } else if ntype == DT_REG {
                self.dump_file(&target_path.as_ref().join(output_path), *i)?;
            }
        }

        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<Jffs2Entry>> {
        let mut jffs2_entries = vec![];
        for i in self.dirents.keys() {
            let (output_path, ntype) = self.resolve_dirent(*i)?;
            if ntype == DT_DIR {
                let entry = Jffs2Entry {
                    inodes: vec![],
                    is_file: false,
                    path: output_path.clone(),
                };
                jffs2_entries.push(entry);
            } else if ntype == DT_REG {
                let inodes = match self.inodes.get(i) {
                    Some(sorted_inodes) => sorted_inodes.to_owned(),
                    _ => vec![],
                };

                let entry = Jffs2Entry {
                    inodes,
                    is_file: true,
                    path: output_path.clone(),
                };
                jffs2_entries.push(entry);
            }
        }

        Ok(jffs2_entries)
    }
}

/// extract the data from a jffs2 file
/// input : the jffs2 file
/// output : the output path
pub fn extract_jffs2(input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<()> {
    let mut reader = Jffs2Reader::new(input)?;
    reader.scan()?;
    reader.dump(output)
}

/// List all entries within the jffs2 image
pub fn list_jffs2(input: impl AsRef<Path>) -> Result<Vec<Jffs2Entry>> {
    let mut reader = Jffs2Reader::new(input)?;
    reader.scan()?;
    reader.entries()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_extract_jffs2() {
        let input = Path::new("test/test.jffs2");
        let mut reader = Jffs2Reader::new(input).expect("Failed to open file");
        reader.scan().expect("Failed to scan");
    }
}
