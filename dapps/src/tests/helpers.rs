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

use std::env;
use std::io::{Read, Write};
use std::str::{self, Lines};
use std::sync::Arc;
use std::net::TcpStream;
use rustc_serialize::hex::{ToHex, FromHex};

use ServerBuilder;
use Server;
use apps::urlhint::ContractClient;
use util::{Bytes, Address, Mutex, ToPretty};

const REGISTRAR: &'static str = "8e4e9b13d4b45cb0befc93c3061b1408f67316b2";
const URLHINT: &'static str = "deadbeefcafe0000000000000000000000000000";

pub struct FakeRegistrar {
	pub calls: Arc<Mutex<Vec<(String, String)>>>,
	pub responses: Mutex<Vec<Result<Bytes, String>>>,
}

impl FakeRegistrar {
	fn new() -> Self {
		FakeRegistrar {
			calls: Arc::new(Mutex::new(Vec::new())),
			responses: Mutex::new(
				vec![
					Ok(format!("000000000000000000000000{}", URLHINT).from_hex().unwrap()),
					Ok(Vec::new())
				]
			),
		}
	}
}

impl ContractClient for FakeRegistrar {
	fn registrar(&self) -> Result<Address, String> {
		Ok(REGISTRAR.parse().unwrap())
	}

	fn call(&self, address: Address, data: Bytes) -> Result<Bytes, String> {
		self.calls.lock().push((address.to_hex(), data.to_hex()));
		self.responses.lock().remove(0)
	}
}

pub fn serve_hosts(hosts: Option<Vec<String>>) -> Server {
	let registrar = Arc::new(FakeRegistrar::new());
	let mut dapps_path = env::temp_dir();
	dapps_path.push("non-existent-dir-to-prevent-fs-files-from-loading");
	let builder = ServerBuilder::new(dapps_path.to_str().unwrap().into(), registrar);
	builder.start_unsecured_http(&"127.0.0.1:0".parse().unwrap(), hosts).unwrap()
}

pub fn serve_with_auth(user: &str, pass: &str) -> Server {
	let registrar = Arc::new(FakeRegistrar::new());
	let builder = ServerBuilder::new(env::temp_dir().to_str().unwrap().into(), registrar);
	builder.start_basic_auth_http(&"127.0.0.1:0".parse().unwrap(), None, user, pass).unwrap()
}

pub fn serve() -> Server {
	serve_hosts(None)
}

pub struct Response {
	pub status: String,
	pub headers: Vec<String>,
	pub headers_raw: String,
	pub body: String,
}

pub fn read_block(lines: &mut Lines, all: bool) -> String {
	let mut block = String::new();
	loop {
		let line = lines.next();
		match line {
			None => break,
			Some("") if !all => break,
			Some(v) => {
				block.push_str(v);
				block.push_str("\n");
			},
		}
	}
	block
}

pub fn request(server: Server, request: &str) -> Response {
	let mut req = TcpStream::connect(server.addr()).unwrap();
	req.write_all(request.as_bytes()).unwrap();

	let mut response = String::new();
	req.read_to_string(&mut response).unwrap();

	let mut lines = response.lines();
	let status = lines.next().unwrap().to_owned();
	let headers_raw = read_block(&mut lines, false);
	let headers = headers_raw.split('\n').map(|v| v.to_owned()).collect();
	let body = read_block(&mut lines, true);

	Response {
		status: status,
		headers: headers,
		headers_raw: headers_raw,
		body: body,
	}
}

