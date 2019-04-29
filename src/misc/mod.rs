pub mod http;
pub mod timer;
pub mod tmp_image_access;

pub use self::http::get_bytes;
pub use self::timer::Timer;
pub use self::tmp_image_access::TmpImageViewAccess;
use std::sync::Arc;

pub fn do_work<W: Send + 'static>(work: Vec<W>, func: Arc<Fn(W) + Send + Sync>) {
	let threads = ::num_cpus::get();
	let mut split = Vec::new();
	
	for _ in 0..threads {
		split.push(Vec::new());
	}
	
	let mut t = 0;
	
	for w in work {
		split[t].push(w);
		t += 1;
		
		if t >= threads {
			t = 0;
		}
	}
	
	let mut handles = Vec::new();
	
	for tw in split {
		let f = func.clone();
		handles.push(::thread::spawn(move || {
			for w in tw {
				f(w);
			}
		}));
	}
	
	for handle in handles {
		let _ = handle.join();
	}
}

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

