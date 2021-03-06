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

//! Ethcore Webapplications for Parity
//! ```
//! extern crate jsonrpc_core;
//! extern crate ethcore_dapps;
//!
//! use std::sync::Arc;
//! use jsonrpc_core::IoHandler;
//! use ethcore_dapps::*;
//!
//! struct SayHello;
//! impl MethodCommand for SayHello {
//! 	fn execute(&self, _params: Params) -> Result<Value, Error> {
//! 		Ok(Value::String("hello".to_string()))
//! 	}
//! }
//!
//! fn main() {
//! 	let io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//! 	let _server = Server::start_unsecure_http(
//! 		&"127.0.0.1:3030".parse().unwrap(),
//! 		Arc::new(io)
//! 	);
//! }
//! ```
//!
#![warn(missing_docs)]
#![cfg_attr(feature="nightly", plugin(clippy))]

#[macro_use]
extern crate log;
extern crate url as url_lib;
extern crate hyper;
extern crate unicase;
extern crate serde;
extern crate serde_json;
extern crate zip;
extern crate rand;
extern crate ethabi;
extern crate jsonrpc_core;
extern crate jsonrpc_http_server;
extern crate mime_guess;
extern crate rustc_serialize;
extern crate parity_dapps;
extern crate https_fetch;
extern crate ethcore_rpc;
extern crate ethcore_util as util;
extern crate linked_hash_map;

mod endpoint;
mod apps;
mod page;
mod router;
mod handlers;
mod rpc;
mod api;
mod proxypac;
mod url;
#[cfg(test)]
mod tests;

pub use self::apps::urlhint::ContractClient;

use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use std::collections::HashMap;

use jsonrpc_core::{IoHandler, IoDelegate};
use router::auth::{Authorization, NoAuth, HttpBasicAuth};
use ethcore_rpc::Extendable;

static DAPPS_DOMAIN : &'static str = ".parity";

/// Webapps HTTP+RPC server build.
pub struct ServerBuilder {
	dapps_path: String,
	handler: Arc<IoHandler>,
	registrar: Arc<ContractClient>,
}

impl Extendable for ServerBuilder {
	fn add_delegate<D: Send + Sync + 'static>(&self, delegate: IoDelegate<D>) {
		self.handler.add_delegate(delegate);
	}
}

impl ServerBuilder {
	/// Construct new dapps server
	pub fn new(dapps_path: String, registrar: Arc<ContractClient>) -> Self {
		ServerBuilder {
			dapps_path: dapps_path,
			handler: Arc::new(IoHandler::new()),
			registrar: registrar,
		}
	}

	/// Asynchronously start server with no authentication,
	/// returns result with `Server` handle on success or an error.
	pub fn start_unsecured_http(&self, addr: &SocketAddr, hosts: Option<Vec<String>>) -> Result<Server, ServerError> {
		Server::start_http(
			addr,
			hosts,
			NoAuth,
			self.handler.clone(),
			self.dapps_path.clone(),
			self.registrar.clone()
		)
	}

	/// Asynchronously start server with `HTTP Basic Authentication`,
	/// return result with `Server` handle on success or an error.
	pub fn start_basic_auth_http(&self, addr: &SocketAddr, hosts: Option<Vec<String>>, username: &str, password: &str) -> Result<Server, ServerError> {
		Server::start_http(
			addr,
			hosts,
			HttpBasicAuth::single_user(username, password),
			self.handler.clone(),
			self.dapps_path.clone(),
			self.registrar.clone()
		)
	}
}

/// Webapps HTTP server.
pub struct Server {
	server: Option<hyper::server::Listening>,
	panic_handler: Arc<Mutex<Option<Box<Fn() -> () + Send>>>>,
}

impl Server {
	/// Returns a list of allowed hosts or `None` if all hosts are allowed.
	fn allowed_hosts(hosts: Option<Vec<String>>, bind_address: String) -> Option<Vec<String>> {
		let mut allowed = Vec::new();

		match hosts {
			Some(hosts) => allowed.extend_from_slice(&hosts),
			None => return None,
		}

		// Add localhost domain as valid too if listening on loopback interface.
		allowed.push(bind_address.replace("127.0.0.1", "localhost").into());
		allowed.push(bind_address.into());
		Some(allowed)
	}

	fn start_http<A: Authorization + 'static>(
		addr: &SocketAddr,
		hosts: Option<Vec<String>>,
		authorization: A,
		handler: Arc<IoHandler>,
		dapps_path: String,
		registrar: Arc<ContractClient>,
	) -> Result<Server, ServerError> {
		let panic_handler = Arc::new(Mutex::new(None));
		let authorization = Arc::new(authorization);
		let apps_fetcher = Arc::new(apps::fetcher::AppFetcher::new(apps::urlhint::URLHintContract::new(registrar)));
		let endpoints = Arc::new(apps::all_endpoints(dapps_path));
		let special = Arc::new({
			let mut special = HashMap::new();
			special.insert(router::SpecialEndpoint::Rpc, rpc::rpc(handler, panic_handler.clone()));
			special.insert(router::SpecialEndpoint::Api, api::RestApi::new(format!("{}", addr), endpoints.clone()));
			special.insert(router::SpecialEndpoint::Utils, apps::utils());
			special
		});
		let hosts = Self::allowed_hosts(hosts, format!("{}", addr));

		try!(hyper::Server::http(addr))
			.handle(move |ctrl| router::Router::new(
				ctrl,
				apps::main_page(),
				apps_fetcher.clone(),
				endpoints.clone(),
				special.clone(),
				authorization.clone(),
				hosts.clone(),
			))
			.map(|(l, srv)| {

				::std::thread::spawn(move || {
					srv.run();
				});

				Server {
					server: Some(l),
					panic_handler: panic_handler,
				}
			})
			.map_err(ServerError::from)
	}

	/// Set callback for panics.
	pub fn set_panic_handler<F>(&self, handler: F) where F : Fn() -> () + Send + 'static {
		*self.panic_handler.lock().unwrap() = Some(Box::new(handler));
	}

	#[cfg(test)]
	/// Returns address that this server is bound to.
	pub fn addr(&self) -> &SocketAddr {
		self.server.as_ref().expect("server is always Some at the start; it's consumed only when object is dropped; qed").addr()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close()
	}
}

/// Webapp Server startup error
#[derive(Debug)]
pub enum ServerError {
	/// Wrapped `std::io::Error`
	IoError(std::io::Error),
	/// Other `hyper` error
	Other(hyper::error::Error),
}

impl From<hyper::error::Error> for ServerError {
	fn from(err: hyper::error::Error) -> Self {
		match err {
			hyper::error::Error::Io(e) => ServerError::IoError(e),
			e => ServerError::Other(e),
		}
	}
}

/// Random filename
pub fn random_filename() -> String {
	use ::rand::Rng;
	let mut rng = ::rand::OsRng::new().unwrap();
	rng.gen_ascii_chars().take(12).collect()
}

#[cfg(test)]
mod util_tests {
	use super::Server;

	#[test]
	fn should_return_allowed_hosts() {
		// given
		let bind_address = "127.0.0.1".to_owned();

		// when
		let all = Server::allowed_hosts(None, bind_address.clone());
		let address = Server::allowed_hosts(Some(Vec::new()), bind_address.clone());
		let some = Server::allowed_hosts(Some(vec!["ethcore.io".into()]), bind_address.clone());

		// then
		assert_eq!(all, None);
		assert_eq!(address, Some(vec!["localhost".into(), "127.0.0.1".into()]));
		assert_eq!(some, Some(vec!["ethcore.io".into(), "localhost".into(), "127.0.0.1".into()]));
	}
}
