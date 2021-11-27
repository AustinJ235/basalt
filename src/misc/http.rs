pub fn get_bytes<U: AsRef<str>>(url: U) -> Result<Vec<u8>, String> {
	let mut handle = curl::easy::Easy::new();
	handle.follow_location(true).unwrap();

	let mut bytes = Vec::new();
	handle.url(url.as_ref()).map_err(|e| format!("bad url: {}", e))?;

	{
		let mut transfer = handle.transfer();
		transfer
			.write_function(|new_data| {
				bytes.extend_from_slice(new_data);
				Ok(new_data.len())
			})
			.map_err(|e| format!("write function: {}", e))?;
		transfer.perform().map_err(|e| format!("failed to perform: {}", e))?;
	}

	Ok(bytes)
}
