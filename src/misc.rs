use std::collections::{BTreeMap,HashMap};
use std::hash::Hash;

pub fn partial_ord_min<T: PartialOrd>(v1: T, v2: T) -> T {
	if v1 < v2 {
		v1
	} else {
		v2
	}
}

pub fn partial_ord_min3<T: PartialOrd>(v1: T, v2: T, v3: T) -> T {
	partial_ord_min(partial_ord_min(v1, v2), v3)
}

pub fn partial_ord_max<T: PartialOrd>(v1: T, v2: T) -> T {
	if v1 > v2 {
		v1
	} else {
		v2
	}
}

pub fn partial_ord_max3<T: PartialOrd>(v1: T, v2: T, v3: T) -> T {
	partial_ord_max(partial_ord_max(v1, v2), v3)
}

pub trait HashMapExtras<K: Eq, V> {
	fn get_mut_or_create(&mut self, key: &K, val: V) -> &mut V;
	fn get_mut_or_else<F: FnMut() -> V>(&mut self, key: &K, func: F) -> &mut V;
	fn get_mut_or_else_with_error<E, F: FnMut() -> Result<V, E>>(&mut self, key: &K, func: F) -> Result<&mut V, E>;
}

impl<K, V> HashMapExtras<K, V> for HashMap<K, V> where K: Eq + Hash + Clone {
	fn get_mut_or_create(&mut self, key: &K, val: V) -> &mut V {
		unsafe {
			let ptr = self as *mut Self;
			let ref mut this1 = *ptr;
			let ref mut this2 = *ptr;
	
			match this1.get_mut(key) {
				Some(some) => some,
				None => {
					this2.insert(key.clone(), val);
					this2.get_mut(key).unwrap()
				}
			}
		}
	}
	
	fn get_mut_or_else<F: FnMut() -> V>(&mut self, key: &K, mut func: F) -> &mut V {
		unsafe {
			let ptr = self as *mut Self;
			let ref mut this1 = *ptr;
			let ref mut this2 = *ptr;
	
			match this1.get_mut(key) {
				Some(some) => some,
				None => {
					this2.insert(key.clone(), func());
					this2.get_mut(key).unwrap()
				}
			}
		}
	}
	
	fn get_mut_or_else_with_error<E, F: FnMut() -> Result<V, E>>(&mut self, key: &K, mut func: F) -> Result<&mut V, E> {
		Ok(unsafe {
			let ptr = self as *mut Self;
			let ref mut this1 = *ptr;
			let ref mut this2 = *ptr;
	
			match this1.get_mut(key) {
				Some(some) => some,
				None => {
					this2.insert(key.clone(), match func() {
						Ok(ok) => ok,
						Err(e) => return Err(e)
					}); this2.get_mut(key).unwrap()
				}
			}
		})
	}
}

pub trait BTreeMapExtras<K: Ord, V> {
	fn get_mut_or_create(&mut self, key: &K, val: V) -> &mut V;
	fn get_mut_or_else<F: FnMut() -> V>(&mut self, key: &K, func: F) -> &mut V;
	fn get_mut_or_else_with_error<E, F: FnMut() -> Result<V, E>>(&mut self, key: &K, func: F) -> Result<&mut V, E>;
}

impl<K, V> BTreeMapExtras<K, V> for BTreeMap<K, V> where K: Ord + Clone {
	fn get_mut_or_create(&mut self, key: &K, val: V) -> &mut V {
		unsafe {
			let ptr = self as *mut Self;
			let ref mut this1 = *ptr;
			let ref mut this2 = *ptr;
	
			match this1.get_mut(key) {
				Some(some) => some,
				None => {
					this2.insert(key.clone(), val);
					this2.get_mut(key).unwrap()
				}
			}
		}
	}
	
	fn get_mut_or_else<F: FnMut() -> V>(&mut self, key: &K, mut func: F) -> &mut V {
		unsafe {
			let ptr = self as *mut Self;
			let ref mut this1 = *ptr;
			let ref mut this2 = *ptr;
	
			match this1.get_mut(key) {
				Some(some) => some,
				None => {
					this2.insert(key.clone(), func());
					this2.get_mut(key).unwrap()
				}
			}
		}
	}
	
	fn get_mut_or_else_with_error<E, F: FnMut() -> Result<V, E>>(&mut self, key: &K, mut func: F) -> Result<&mut V, E> {
		Ok(unsafe {
			let ptr = self as *mut Self;
			let ref mut this1 = *ptr;
			let ref mut this2 = *ptr;
	
			match this1.get_mut(key) {
				Some(some) => some,
				None => {
					this2.insert(key.clone(), match func() {
						Ok(ok) => ok,
						Err(e) => return Err(e)
					}); this2.get_mut(key).unwrap()
				}
			}
		})
	}
}

