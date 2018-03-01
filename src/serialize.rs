use serde::{Serialize,Deserialize};
use bincode;
use std::path::Path;
use std::fs::File;

pub fn to_file<S: Serialize, P: AsRef<Path>>(source: &S, to: P) -> Result<(), String> {
	let mut handle = match File::create(to) {
		Ok(ok) => ok,
		Err(e) => return Err(format!("Failed to open file: {}", e))
	}; match bincode::serialize_into(&mut handle, source, bincode::Infinite) {
		Ok(_) => Ok(()),
		Err(e) => Err(format!("Bincode serialize_into error: {}", e))
	}
}

pub fn from_file<T, P: AsRef<Path>>(from: P) -> Result<T, String>
	where for<'de> T: Deserialize<'de> {
		
	let mut handle = match File::open(from) {
		Ok(ok) => ok,
		Err(e) => return Err(format!("Failed to open file: {}", e))
	}; let ok: T = match bincode::deserialize_from(&mut handle, bincode::Infinite) {
		Ok(ok) => ok,
		Err(e) => return Err(format!("{}", e))
	}; Ok(ok)
}

