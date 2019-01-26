use core::{
    mem,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::Pool;

#[test]
fn sanity() {
    static POOL: Pool<u8> = Pool::new();
    #[cfg(not(feature = "union"))]
    static mut MEMORY: [u8; 31] = [0; 31];
    #[cfg(feature = "union")]
    static mut MEMORY: [u8; 15] = [0; 15];

    // empty pool
    assert!(POOL.alloc().is_none());

    POOL.grow(unsafe { &mut MEMORY });

    let x = POOL.alloc().unwrap().init(0);
    assert_eq!(*x, 0);

    // pool exhausted
    assert!(POOL.alloc().is_none());

    POOL.free(x);

    // should be possible to allocate again
    assert_eq!(*POOL.alloc().unwrap().init(1), 1);
}

#[test]
fn destructors() {
    static COUNT: AtomicUsize = AtomicUsize::new(0);

    struct X;

    impl X {
        fn new() -> X {
            COUNT.fetch_add(1, Ordering::Relaxed);
            X
        }
    }

    impl Drop for X {
        fn drop(&mut self) {
            COUNT.fetch_sub(1, Ordering::Relaxed);
        }
    }

    static POOL: Pool<X> = Pool::new();
    static mut MEMORY: [u8; 31] = [0; 31];

    POOL.grow(unsafe { &mut MEMORY });

    let x = POOL.alloc().unwrap().init(X::new());
    let y = POOL.alloc().unwrap().init(X::new());
    let z = POOL.alloc().unwrap().init(X::new());

    assert_eq!(COUNT.load(Ordering::Relaxed), 3);

    // this leaks memory
    drop(x);

    assert_eq!(COUNT.load(Ordering::Relaxed), 3);

    // this leaks memory
    mem::forget(y);

    assert_eq!(COUNT.load(Ordering::Relaxed), 3);

    // this runs `X` destructor
    POOL.free(z);

    assert_eq!(COUNT.load(Ordering::Relaxed), 2);
}
