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

//! Parity sync service

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use hypervisor::{SYNC_MODULE_ID, HYPERVISOR_IPC_URL, ControlService};
use ethcore::client::{RemoteClient, ChainNotify};
use ethsync::{SyncProvider, EthSync, ManageNetwork, ServiceConfiguration};
use modules::service_urls;
use boot;
use nanoipc;

#[derive(Default)]
struct SyncControlService {
	pub stop: Arc<AtomicBool>,
}

impl ControlService for SyncControlService {
	fn shutdown(&self) -> bool {
		trace!(target: "hypervisor", "Received shutdown from control service");
		self.stop.store(true, ::std::sync::atomic::Ordering::SeqCst);
		true
	}
}

pub fn main() {
	boot::setup_cli_logger("sync");

	let service_config: ServiceConfiguration = boot::payload()
		.unwrap_or_else(|e| panic!("Fatal: error reading boot arguments ({:?})", e));

	let remote_client = dependency!(RemoteClient, &service_urls::with_base(&service_config.io_path, service_urls::CLIENT));

	let sync = EthSync::new(service_config.sync, remote_client.service().clone(), service_config.net).unwrap();

	let _ = boot::main_thread();
	let service_stop = Arc::new(AtomicBool::new(false));

	let hypervisor = boot::register(
		&service_urls::with_base(&service_config.io_path, HYPERVISOR_IPC_URL),
		&service_urls::with_base(&service_config.io_path, service_urls::SYNC_CONTROL),
		SYNC_MODULE_ID
	);

	boot::host_service(
		&service_urls::with_base(&service_config.io_path, service_urls::SYNC),
		service_stop.clone(),
		sync.clone() as Arc<SyncProvider>
	);
	boot::host_service(
		&service_urls::with_base(&service_config.io_path, service_urls::NETWORK_MANAGER),
		service_stop.clone(),
		sync.clone() as Arc<ManageNetwork>
	);
	boot::host_service(
		&service_urls::with_base(&service_config.io_path, service_urls::SYNC_NOTIFY),
		service_stop.clone(),
		sync.clone() as Arc<ChainNotify>
	);

	let control_service = Arc::new(SyncControlService::default());
	let as_control = control_service.clone() as Arc<ControlService>;
	let mut worker = nanoipc::Worker::<ControlService>::new(&as_control);
	let thread_stop = control_service.stop.clone();
	worker.add_reqrep(
		&service_urls::with_base(&service_config.io_path, service_urls::SYNC_CONTROL)
	).unwrap();

	while !thread_stop.load(::std::sync::atomic::Ordering::SeqCst) {
		worker.poll();
	}
	service_stop.store(true, ::std::sync::atomic::Ordering::SeqCst);

	hypervisor.module_shutdown(SYNC_MODULE_ID);
	trace!(target: "hypervisor", "Sync process terminated gracefully");
}
