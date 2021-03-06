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

//! Ethcore client application.

#![warn(missing_docs)]
#![cfg_attr(feature="dev", feature(plugin))]
#![cfg_attr(feature="dev", plugin(clippy))]
#![cfg_attr(feature="dev", allow(useless_format))]
#![cfg_attr(feature="dev", allow(match_bool))]

extern crate docopt;
extern crate num_cpus;
extern crate rustc_serialize;
extern crate ethcore_devtools as devtools;
extern crate ethcore;
extern crate ethsync;
extern crate env_logger;
extern crate ethcore_logger;
extern crate ctrlc;
extern crate fdlimit;
extern crate time;
extern crate number_prefix;
extern crate rpassword;
extern crate semver;
extern crate ethcore_io as io;
extern crate ethcore_ipc as ipc;
extern crate ethcore_ipc_nano as nanoipc;
extern crate rlp;

extern crate json_ipc_server as jsonipc;

extern crate ethcore_ipc_hypervisor as hypervisor;
extern crate ethcore_rpc;

extern crate ethcore_signer;
extern crate ansi_term;

extern crate regex;
extern crate isatty;

#[macro_use]
extern crate ethcore_util as util;
#[macro_use]
extern crate log as rlog;
#[macro_use]
extern crate hyper; // for price_info.rs
#[macro_use]
extern crate lazy_static;

#[cfg(feature="stratum")]
extern crate ethcore_stratum;

#[cfg(feature = "dapps")]
extern crate ethcore_dapps;

macro_rules! dependency {
	($dep_ty:ident, $url:expr) => {
		{
			let dep = boot::dependency::<$dep_ty<_>>($url)
				.unwrap_or_else(|e| panic!("Fatal: error connecting service ({:?})", e));
			dep.handshake()
				.unwrap_or_else(|e| panic!("Fatal: error in connected service ({:?})", e));
			dep
		}
	}
}

mod cache;
mod upgrade;
mod rpc;
mod dapps;
mod informant;
mod io_handler;
mod cli;
mod configuration;
mod migration;
mod signer;
mod rpc_apis;
mod url;
mod helpers;
mod params;
mod deprecated;
mod dir;
mod modules;
mod account;
mod blockchain;
mod presale;
mod run;
mod sync;
mod snapshot;
mod boot;

#[cfg(feature="stratum")]
mod stratum;

use std::{process, env};
use cli::print_version;
use configuration::{Cmd, Configuration};
use deprecated::find_deprecated;

fn execute(command: Cmd) -> Result<String, String> {
	match command {
		Cmd::Run(run_cmd) => {
			try!(run::execute(run_cmd));
			Ok("".into())
		},
		Cmd::Version => Ok(print_version()),
		Cmd::Account(account_cmd) => account::execute(account_cmd),
		Cmd::ImportPresaleWallet(presale_cmd) => presale::execute(presale_cmd),
		Cmd::Blockchain(blockchain_cmd) => blockchain::execute(blockchain_cmd),
		Cmd::SignerToken(path) => signer::new_token(path),
		Cmd::Snapshot(snapshot_cmd) => snapshot::execute(snapshot_cmd),
	}
}

fn start() -> Result<String, String> {
	let conf = Configuration::parse(env::args()).unwrap_or_else(|e| e.exit());

	let deprecated = find_deprecated(&conf.args);
	for d in deprecated {
		println!("{}", d);
	}

	let cmd = try!(conf.into_command());
	execute(cmd)
}

#[cfg(feature="stratum")]
mod stratum_optional {
	pub fn probably_run() -> bool {
		// just redirect to the stratum::main()
		if ::std::env::args().nth(1).map_or(false, |arg| arg == "stratum") {
			super::stratum::main();
			true
		}
		else { false }
	}
}

#[cfg(not(feature="stratum"))]
mod stratum_optional {
	pub fn probably_run() -> bool {
		false
	}
}

fn main() {
	// just redirect to the sync::main()
	if std::env::args().nth(1).map_or(false, |arg| arg == "sync") {
		sync::main();
		return;
	}

	if stratum_optional::probably_run() { return; }

	match start() {
		Ok(result) => {
			println!("{}", result);
		},
		Err(err) => {
			println!("{}", err);
			process::exit(1);
		}
	}
}
