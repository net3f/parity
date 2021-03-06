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


use DAPPS_DOMAIN;
use hyper::{server, header};
use hyper::net::HttpStream;

use jsonrpc_http_server::{is_host_header_valid};
use handlers::ContentHandler;

pub fn is_valid(request: &server::Request<HttpStream>, allowed_hosts: &[String], endpoints: Vec<String>) -> bool {
	let mut endpoints = endpoints.iter()
		.map(|endpoint| format!("{}{}", endpoint, DAPPS_DOMAIN))
		.collect::<Vec<String>>();
	endpoints.extend_from_slice(allowed_hosts);

	let header_valid = is_host_header_valid(request, &endpoints);

	match (header_valid, request.headers().get::<header::Host>()) {
		(true, _) => true,
		(_, Some(host)) => host.hostname.ends_with(DAPPS_DOMAIN),
		_ => false,
	}
}

pub fn host_invalid_response() -> Box<server::Handler<HttpStream> + Send> {
	Box::new(ContentHandler::forbidden(
		r#"
		<h1>Request with disallowed <code>Host</code> header has been blocked.</h1>
		<p>Check the URL in your browser address bar.</p>
		"#.into(),
		"text/html".into()
	))
}
