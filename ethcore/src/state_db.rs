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

use lru_cache::LruCache;
use util::journaldb::JournalDB;
use util::hash::{H256};
use util::hashdb::HashDB;
use util::{Arc, Address, DBTransaction, UtilError, Mutex, Hashable, BytesConvertable};
use account::Account;
use bloomfilter::{Bloom, BloomJournal};
use util::Database;
use client::DB_COL_ACCOUNT_BLOOM;
use byteorder::{LittleEndian, WriteBytesExt, ReadBytesExt};

const STATE_CACHE_ITEMS: usize = 65536;

struct AccountCache {
	/// DB Account cache. `None` indicates that account is known to be missing.
	accounts: LruCache<Address, Option<Account>>,
}

/// State database abstraction.
/// Manages shared global state cache.
/// A clone of `StateDB` may be created as canonical or not.
/// For canonical clones cache changes are accumulated and applied
/// on commit.
/// For non-canonical clones cache is cleared on commit.
pub struct StateDB {
	db: Box<JournalDB>,
	account_cache: Arc<Mutex<AccountCache>>,
	cache_overlay: Vec<(Address, Option<Account>)>,
	is_canon: bool,
	account_bloom: Arc<Mutex<Bloom>>,
}

pub const ACCOUNT_BLOOM_SPACE: usize = 1048576;
pub const DEFAULT_ACCOUNT_PRESET: usize = 1000000;

pub const ACCOUNT_BLOOM_SPACE_COLUMN: &'static[u8] = b"accounts_bloom";
pub const ACCOUNT_BLOOM_HASHCOUNT_COLUMN: &'static[u8] = b"account_hash_count";

impl StateDB {

	pub fn load_bloom(db: &Database) -> Bloom {
		let hash_count_entry = db.get(DB_COL_ACCOUNT_BLOOM, ACCOUNT_BLOOM_HASHCOUNT_COLUMN)
			.expect("Low-level database error");

		if hash_count_entry.is_none() {
			return Bloom::new(ACCOUNT_BLOOM_SPACE, DEFAULT_ACCOUNT_PRESET);
		}
		let hash_count_bytes = hash_count_entry.unwrap();
		assert_eq!(hash_count_bytes.len(), 1);
		let hash_count = hash_count_bytes[0];

		let mut bloom_parts = vec![0u64; ACCOUNT_BLOOM_SPACE / 8];
		let mut key = vec![0u8; 8];
		let empty = vec![0u8; 8];
		for i in 0..ACCOUNT_BLOOM_SPACE / 8 {
			key.write_u64::<LittleEndian>(i as u64);
			bloom_parts[i] = db.get(DB_COL_ACCOUNT_BLOOM, &key).expect("low-level database error")
				.and_then(|val| Some(val.as_slice().read_u64::<LittleEndian>().expect("fatal: invalid bloom data in bloom ")))
				.unwrap_or(0u64);
		}

		Bloom::from_parts(&bloom_parts, hash_count as u32)
	}

	/// Create a new instance wrapping `JournalDB`
	pub fn new(db: Box<JournalDB>) -> StateDB {
		let bloom = Self::load_bloom(db.backing());
		StateDB {
			db: db,
			account_cache: Arc::new(Mutex::new(AccountCache { accounts: LruCache::new(STATE_CACHE_ITEMS) })),
			cache_overlay: Vec::new(),
			is_canon: false,
			account_bloom: Arc::new(Mutex::new(bloom)),
		}
	}

	fn new_account_bloom() -> Bloom {
		Bloom::new(ACCOUNT_BLOOM_SPACE, DEFAULT_ACCOUNT_PRESET)
	}

	pub fn check_account_bloom(&self, address: &Address) -> bool {
		trace!(target: "state_bloom", "Check account bloom: {:?}", address);
		let bloom = self.account_bloom.lock();
		bloom.check(address.sha3().as_slice())
	}

	pub fn note_account_bloom(&self, address: &Address) {
		trace!(target: "state_bloom", "Note account bloom: {:?}", address);
		let mut bloom = self.account_bloom.lock();
		bloom.set(address.sha3().as_slice());
	}

	pub fn commit_bloom(batch: &DBTransaction, journal: BloomJournal) -> Result<(), UtilError> {
		assert!(journal.hash_functions <= 255);
		try!(batch.put(None, ACCOUNT_BLOOM_HASHCOUNT_COLUMN, &vec![journal.hash_functions as u8]));
		let mut key = vec![0u8; 8];
		let mut val = vec![0u8; 8];

		println!("putting {} bloom entries", journal.entries.len());

		for (bloom_part_index, bloom_part_value) in journal.entries {
			key.write_u64::<LittleEndian>(bloom_part_index as u64).expect("size allocated on stack is enough, therefore this cannot fail");
			val.write_u64::<LittleEndian>(bloom_part_value).expect("size allocated on stack is enough, therefore this cannot fail");
			try!(batch.put(None, &key, &val));
		}
		Ok(())
	}

	/// Commit all recent insert operations and canonical historical commits' removals from the
	/// old era to the backing database, reverting any non-canonical historical commit's inserts.
	pub fn commit(&mut self, batch: &DBTransaction, now: u64, id: &H256, end: Option<(u64, H256)>) -> Result<u32, UtilError> {
		{
			let mut bloom_lock = self.account_bloom.lock();
			Self::commit_bloom(batch, bloom_lock.drain_journal());
		}

		let records = try!(self.db.commit(batch, now, id, end));
		if self.is_canon {
			self.commit_cache();
		} else {
			self.clear_cache();
		}
		Ok(records)
	}

	/// Returns an interface to HashDB.
	pub fn as_hashdb(&self) -> &HashDB {
		self.db.as_hashdb()
	}

	/// Returns an interface to mutable HashDB.
	pub fn as_hashdb_mut(&mut self) -> &mut HashDB {
		self.db.as_hashdb_mut()
	}

	/// Clone the database.
	pub fn boxed_clone(&self) -> StateDB {
		StateDB {
			db: self.db.boxed_clone(),
			account_cache: self.account_cache.clone(),
			cache_overlay: Vec::new(),
			is_canon: false,
			account_bloom: self.account_bloom.clone(),
		}
	}

	/// Clone the database for a canonical state.
	pub fn boxed_clone_canon(&self) -> StateDB {
		StateDB {
			db: self.db.boxed_clone(),
			account_cache: self.account_cache.clone(),
			cache_overlay: Vec::new(),
			is_canon: true,
			account_bloom: self.account_bloom.clone(),
		}
	}

	/// Check if pruning is enabled on the database.
	pub fn is_pruned(&self) -> bool {
		self.db.is_pruned()
	}

	/// Heap size used.
	pub fn mem_used(&self) -> usize {
		self.db.mem_used() //TODO: + self.account_cache.lock().heap_size_of_children()
	}

	/// Returns underlying `JournalDB`.
	pub fn journal_db(&self) -> &JournalDB {
		&*self.db
	}

	/// Enqueue cache change.
	pub fn cache_account(&mut self, addr: Address, data: Option<Account>) {
		self.cache_overlay.push((addr, data));
	}

	/// Apply pending cache changes.
	fn commit_cache(&mut self) {
		let mut cache = self.account_cache.lock();
		for (address, account) in self.cache_overlay.drain(..) {
			if let Some(&mut Some(ref mut existing)) = cache.accounts.get_mut(&address) {
				if let Some(new) = account {
					existing.merge_with(new);
					continue;
				}
			}
			cache.accounts.insert(address, account);
		}
	}

	/// Clear the cache.
	pub fn clear_cache(&mut self) {
		self.cache_overlay.clear();
		let mut cache = self.account_cache.lock();
		cache.accounts.clear();
	}

	/// Get basic copy of the cached account. Does not include storage.
	/// Returns 'None' if the state is non-canonical and cache is disabled
	/// or if the account is not cached.
	pub fn get_cached_account(&self, addr: &Address) -> Option<Option<Account>> {
		if !self.is_canon {
			return None;
		}
		let mut cache = self.account_cache.lock();
		cache.accounts.get_mut(&addr).map(|a| a.as_ref().map(|a| a.clone_basic()))
	}

	/// Get value from a cached account.
	/// Returns 'None' if the state is non-canonical and cache is disabled
	/// or if the account is not cached.
	pub fn get_cached<F, U>(&self, a: &Address, f: F) -> Option<U>
		where F: FnOnce(Option<&mut Account>) -> U {
		if !self.is_canon {
			return None;
		}
		let mut cache = self.account_cache.lock();
		cache.accounts.get_mut(a).map(|c| f(c.as_mut()))
	}
}

