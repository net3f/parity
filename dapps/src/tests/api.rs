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

use tests::helpers::{serve, request};

#[test]
fn should_return_error() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			GET /api/empty HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			\r\n\
			{}
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 404 Not Found".to_owned());
	assert_eq!(response.headers.get(0).unwrap(), "Content-Type: application/json");
	assert_eq!(response.body, format!("58\n{}\n0\n\n", r#"{"code":"404","title":"Not Found","detail":"Resource you requested has not been found."}"#));
}

#[test]
fn should_serve_apps() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			GET /api/apps HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			\r\n\
			{}
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.headers.get(0).unwrap(), "Content-Type: application/json");
	assert!(response.body.contains("Parity Home Screen"));
}

#[test]
fn should_handle_ping() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			POST /api/ping HTTP/1.1\r\n\
			Host: home.parity\r\n\
			Connection: close\r\n\
			\r\n\
			{}
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.headers.get(0).unwrap(), "Content-Type: application/json");
	assert_eq!(response.body, "0\n\n".to_owned());
}

