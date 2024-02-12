pub mod timer;

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::thread;
use std::thread::available_parallelism;

pub use self::timer::Timer;

pub fn drain_filter<T, F: FnMut(&mut T) -> bool>(vec: &mut Vec<T>, mut pred: F) -> Vec<T> {
    let mut i = 0;
    let mut out = Vec::new();

    while i < vec.len() {
        if pred(&mut vec[i]) {
            out.push(vec.remove(i));
        } else {
            i += 1;
        }
    }

    out
}

pub fn do_work<W: Send + 'static>(work: Vec<W>, func: Arc<dyn Fn(W) + Send + Sync>) {
    let threads = available_parallelism()
        .unwrap_or(NonZeroUsize::new(4).unwrap())
        .get();
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
        handles.push(thread::spawn(move || {
            for w in tw {
                f(w);
            }
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }
}
