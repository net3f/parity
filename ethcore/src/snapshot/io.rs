// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

//! Snapshot i/o.
//! Ways of writing and reading snapshots. This module supports writing and reading
//! snapshots of two different formats: packed and loose.
//! Packed snapshots are written to a single file, and loose snapshots are
//! written to multiple files in one directory.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use util::Bytes;
use util::hash::H256;
use rlp::{self, Encodable, RlpStream, UntrustedRlp, Stream, View};

use super::ManifestData;

/// Something which can write snapshots.
/// Writing the same chunk multiple times will lead to implementation-defined
/// behavior, and is not advised.
pub trait SnapshotWriter {
	/// Write a compressed state chunk.
	fn write_state_chunk(&mut self, hash: H256, chunk: &[u8]) -> io::Result<()>;

	/// Write a compressed block chunk.
	fn write_block_chunk(&mut self, hash: H256, chunk: &[u8]) -> io::Result<()>;

	/// Complete writing. The manifest's chunk lists must be consistent
	/// with the chunks written.
	fn finish(self, manifest: ManifestData) -> io::Result<()> where Self: Sized;
}

// (hash, len, offset)
struct ChunkInfo(H256, u64, u64);

impl Encodable for ChunkInfo {
	fn rlp_append(&self, s: &mut RlpStream) {
		s.begin_list(3);
		s.append(&self.0).append(&self.1).append(&self.2);
	}
}

impl rlp::Decodable for ChunkInfo {
	fn decode<D: rlp::Decoder>(decoder: &D) -> Result<Self, rlp::DecoderError> {
		let d = decoder.as_rlp();

		let hash = try!(d.val_at(0));
		let len = try!(d.val_at(1));
		let off = try!(d.val_at(2));
		Ok(ChunkInfo(hash, len, off))
	}
}

/// A packed snapshot writer. This writes snapshots to a single concatenated file.
///
/// The file format is very simple and consists of three parts:
/// 	[Concatenated chunk data]
/// 	[manifest as RLP]
///     [manifest start offset (8 bytes little-endian)]
///
/// The manifest contains all the same information as a standard `ManifestData`,
/// but also maps chunk hashes to their lengths and offsets in the file
/// for easy reading.
pub struct PackedWriter {
	file: File,
	state_hashes: Vec<ChunkInfo>,
	block_hashes: Vec<ChunkInfo>,
	cur_len: u64,
}

impl PackedWriter {
	/// Create a new "PackedWriter", to write into the file at the given path.
	pub fn new(path: &Path) -> io::Result<Self> {
		Ok(PackedWriter {
			file: try!(File::create(path)),
			state_hashes: Vec::new(),
			block_hashes: Vec::new(),
			cur_len: 0,
		})
	}
}

impl SnapshotWriter for PackedWriter {
	fn write_state_chunk(&mut self, hash: H256, chunk: &[u8]) -> io::Result<()> {
		try!(self.file.write_all(chunk));

		let len = chunk.len() as u64;
		self.state_hashes.push(ChunkInfo(hash, len, self.cur_len));

		self.cur_len += len;
		Ok(())
	}

	fn write_block_chunk(&mut self, hash: H256, chunk: &[u8]) -> io::Result<()> {
		try!(self.file.write_all(chunk));

		let len = chunk.len() as u64;
		self.block_hashes.push(ChunkInfo(hash, len, self.cur_len));

		self.cur_len += len;
		Ok(())
	}

	fn finish(mut self, manifest: ManifestData) -> io::Result<()> {
		// we ignore the hashes fields of the manifest under the assumption that
		// they are consistent with ours.
		let mut stream = RlpStream::new_list(5);
		stream
			.append(&self.state_hashes)
			.append(&self.block_hashes)
			.append(&manifest.state_root)
			.append(&manifest.block_number)
			.append(&manifest.block_hash);

		let manifest_rlp = stream.out();

		try!(self.file.write_all(&manifest_rlp));
		let off = self.cur_len;
		trace!(target: "snapshot_io", "writing manifest of len {} to offset {}", manifest_rlp.len(), off);

		let off_bytes: [u8; 8] =
			[
				off as u8,
				(off >> 8) as u8,
				(off >> 16) as u8,
				(off >> 24) as u8,
				(off >> 32) as u8,
				(off >> 40) as u8,
				(off >> 48) as u8,
				(off >> 56) as u8,
			];

		try!(self.file.write_all(&off_bytes[..]));

		Ok(())
	}
}

/// A "loose" writer writes chunk files into a directory.
pub struct LooseWriter {
	dir: PathBuf,
}

impl LooseWriter {
	/// Create a new LooseWriter which will write into the given directory,
	/// creating it if it doesn't exist.
	pub fn new(path: PathBuf) -> io::Result<Self> {
		try!(fs::create_dir_all(&path));

		Ok(LooseWriter {
			dir: path,
		})
	}

	// writing logic is the same for both kinds of chunks.
	fn write_chunk(&mut self, hash: H256, chunk: &[u8]) -> io::Result<()> {
		let mut file_path = self.dir.clone();
		file_path.push(hash.hex());

		let mut file = try!(File::create(file_path));
		try!(file.write_all(chunk));

		Ok(())
	}
}

impl SnapshotWriter for LooseWriter {
	fn write_state_chunk(&mut self, hash: H256, chunk: &[u8]) -> io::Result<()> {
		self.write_chunk(hash, chunk)
	}

	fn write_block_chunk(&mut self, hash: H256, chunk: &[u8]) -> io::Result<()> {
		self.write_chunk(hash, chunk)
	}

	fn finish(self, manifest: ManifestData) -> io::Result<()> {
		let rlp = manifest.into_rlp();
		let mut path = self.dir.clone();
		path.push("MANIFEST");

		let mut file = try!(File::create(path));
		try!(file.write_all(&rlp[..]));

		Ok(())
	}
}

/// Something which can read compressed snapshots.
pub trait SnapshotReader {
	/// Get the manifest data for this snapshot.
	fn manifest(&self) -> &ManifestData;

	/// Get raw chunk data by hash. implementation defined behavior
	/// if a chunk not in the manifest is requested.
	fn chunk(&self, hash: H256) -> io::Result<Bytes>;
}

/// Packed snapshot reader.
pub struct PackedReader {
	file: File,
	state_hashes: HashMap<H256, (u64, u64)>, // len, offset
	block_hashes: HashMap<H256, (u64, u64)>, // len, offset
	manifest: ManifestData,
}

impl PackedReader {
	/// Create a new `PackedReader` for the file at the given path.
	/// This will fail if any io errors are encountered or the file
	/// is not a valid packed snapshot.
	pub fn new(path: &Path) -> Result<Option<Self>, ::error::Error> {
		let mut file = try!(File::open(path));
		let file_len = try!(file.metadata()).len();
		if file_len < 8 {
			// ensure we don't seek before beginning.
			return Ok(None);
		}


		try!(file.seek(SeekFrom::End(-8)));
		let mut off_bytes = [0u8; 8];

		try!(file.read_exact(&mut off_bytes[..]));

		let manifest_off: u64 =
			((off_bytes[7] as u64) << 56) +
			((off_bytes[6] as u64) << 48) +
			((off_bytes[5] as u64) << 40) +
			((off_bytes[4] as u64) << 32) +
			((off_bytes[3] as u64) << 24) +
			((off_bytes[2] as u64) << 16) +
			((off_bytes[1] as u64) << 8) +
			(off_bytes[0] as u64);

		let manifest_len = file_len - manifest_off - 8;
		trace!(target: "snapshot", "loading manifest of length {} from offset {}", manifest_len, manifest_off);

		let	mut manifest_buf = vec![0; manifest_len as usize];

		try!(file.seek(SeekFrom::Start(manifest_off)));
		try!(file.read_exact(&mut manifest_buf));

		let rlp = UntrustedRlp::new(&manifest_buf);

		let state: Vec<ChunkInfo> = try!(rlp.val_at(0));
		let blocks: Vec<ChunkInfo> = try!(rlp.val_at(1));

		let manifest = ManifestData {
			state_hashes: state.iter().map(|c| c.0).collect(),
			block_hashes: blocks.iter().map(|c| c.0).collect(),
			state_root: try!(rlp.val_at(2)),
			block_number: try!(rlp.val_at(3)),
			block_hash: try!(rlp.val_at(4)),
		};

		Ok(Some(PackedReader {
			file: file,
			state_hashes: state.into_iter().map(|c| (c.0, (c.1, c.2))).collect(),
			block_hashes: blocks.into_iter().map(|c| (c.0, (c.1, c.2))).collect(),
			manifest: manifest
		}))
	}
}

impl SnapshotReader for PackedReader {
	fn manifest(&self) -> &ManifestData {
		&self.manifest
	}

	fn chunk(&self, hash: H256) -> io::Result<Bytes> {
		let &(len, off) = self.state_hashes.get(&hash).or_else(|| self.block_hashes.get(&hash))
			.expect("only chunks in the manifest can be requested; qed");

		let mut file = &self.file;

		try!(file.seek(SeekFrom::Start(off)));
		let mut buf = vec![0; len as usize];

		try!(file.read_exact(&mut buf[..]));

		Ok(buf)
	}
}

/// reader for "loose" snapshots
pub struct LooseReader {
	dir: PathBuf,
	manifest: ManifestData,
}

impl LooseReader {
	/// Create a new `LooseReader` which will read the manifest and chunk data from
	/// the given directory.
	pub fn new(mut dir: PathBuf) -> Result<Self, ::error::Error> {
		let mut manifest_buf = Vec::new();

		dir.push("MANIFEST");
		let mut manifest_file = try!(File::open(&dir));
		try!(manifest_file.read_to_end(&mut manifest_buf));

		let manifest = try!(ManifestData::from_rlp(&manifest_buf[..]));

		dir.pop();

		Ok(LooseReader {
			dir: dir,
			manifest: manifest,
		})
	}
}

impl SnapshotReader for LooseReader {
	fn manifest(&self) -> &ManifestData {
		&self.manifest
	}

	fn chunk(&self, hash: H256) -> io::Result<Bytes> {
		let mut path = self.dir.clone();
		path.push(hash.hex());

		let mut buf = Vec::new();
		let mut file = try!(File::open(&path));

		try!(file.read_to_end(&mut buf));

		Ok(buf)
	}
}