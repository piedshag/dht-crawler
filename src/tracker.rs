use tokio::net::UdpSocket;
use std::io;

#[derive(Debug)]
enum TrackerType {
	UDP,
	Unknown,
}

pub struct Tracker {
	address: String,
	protocol: TrackerType,
}

fn determine_type(address: &String) -> TrackerType {
	let protocol = address.split("%3A%").nth(0).expect("Invalid tracker address");
	match protocol {
		"udp" => TrackerType::UDP,
		_ => TrackerType::Unknown,
	}
}

impl Tracker {
	pub fn new(address: &String) -> Self {
		let protocol = determine_type(&address);
		return Tracker {
			address: address.clone(),
			protocol: protocol,
		}
	}

	pub async fn connect(&self) -> Result<(), io::Error> {
		match self.protocol {
			TrackerType::UDP => self.udp_connect().await,
			TrackerType::Unknown => Err(io::Error::new(io::ErrorKind::Other, "Unsupported")),
		}
	}

	async fn udp_connect(&self) -> Result<(), io::Error> {
		let sock = UdpSocket::bind("0.0.0.0:0").await?;

		println!("{:?}", sock.local_addr().unwrap());

		sock.connect(&self.address).await?;

		Ok(())
	}
}
