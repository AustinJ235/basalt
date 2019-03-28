#[allow(warnings)]

use super::*;
use std::hash::Hash;

pub struct LeaseMap<K: Hash, V: Clone> {
	inner: Vec<(K, Leaser<V>)>,
}

impl<K: Hash, V: Clone> LeaseMap<K, V> {
	pub fn new() -> Self {
		unimplemented!()
	}

	pub fn get_lease(&self, key: &K) -> Option<Lessee<V>> {
		unimplemented!()
	}
	
	pub fn get(&self, key: &K) -> V {
		unimplemented!()
	}
	
	pub fn leases(&self, key: &K) -> usize {
		unimplemented!()
	}
	
	pub fn try_insert(&self, key: K, val: V) -> Option<(K, V, String)> {
		unimplemented!()
	}
}

