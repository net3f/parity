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

use tests::helpers::{serve_hosts, request};

#[test]
fn should_reject_invalid_host() {
	// given
	let server = serve_hosts(Some(vec!["localhost:8080".into()]));

	// when
	let response = request(server,
		"\
			GET / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			\r\n\
			{}
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, "85\n\n\t\t<h1>Request with disallowed <code>Host</code> header has been blocked.</h1>\n\t\t<p>Check the URL in your browser address bar.</p>\n\t\t\n0\n\n".to_owned());
}

#[test]
fn should_allow_valid_host() {
	// given
	let server = serve_hosts(Some(vec!["localhost:8080".into()]));

	// when
	let response = request(server,
		"\
			GET /home/ HTTP/1.1\r\n\
			Host: localhost:8080\r\n\
			Connection: close\r\n\
			\r\n\
			{}
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
}

#[test]
fn should_serve_dapps_domains() {
	// given
	let server = serve_hosts(Some(vec!["localhost:8080".into()]));

	// when
	let response = request(server,
		"\
			GET / HTTP/1.1\r\n\
			Host: home.parity\r\n\
			Connection: close\r\n\
			\r\n\
			{}
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
}

