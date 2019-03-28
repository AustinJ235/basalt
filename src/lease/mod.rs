pub mod map;
pub use self::map::LeaseMap;

use std::sync::atomic::{self,AtomicUsize};
use std::sync::Arc;

pub struct Leaser<T: Clone> {
	inner: T,
	leases: Arc<AtomicUsize>,
}

impl<T: Clone> Leaser<T> {
	pub fn new(inner: T) -> Self {
		Leaser {
			inner,
			leases: Arc::new(AtomicUsize::new(0)),
		}
	}
	
	pub fn active_leases(&self) {
		self.leases.load(atomic::Ordering::SeqCst);
	}
	
	pub fn lease(&self) -> Lessee<T> {
		self.leases.fetch_add(1, atomic::Ordering::SeqCst);
	
		Lessee {
			leases: self.leases.clone(),
			inner: self.inner.clone()
		}
	}
}

pub struct Lessee<T> {
	leases: Arc<AtomicUsize>,
	inner: T
}

impl<T> Drop for Lessee<T> {
	fn drop(&mut self) {
		self.leases.fetch_sub(1, atomic::Ordering::SeqCst);
	}
}

impl<T> ::std::ops::Deref for Lessee<T> {
	type Target = T;

	#[inline]
	fn deref(&self) -> &T {
		&self.inner
	}
}

