//! A heap-less, interrupt-safe, lock-free memory pool for Cortex-M devices
//!
//! **WARNING** Do not use this on multi-core devices. The implementation has not been revised for
//! multi-core soundness.
//!
//! **NOTE** This is more likely to be merged into `heapless` (once I iterate on it more) than to
//! be published as its own crate.
//!
//! (You may also be interested in my [old experiments] with allocators and (owned) singletons).
//!
//! [old experiments]: https://docs.rs/alloc-singleton/0.1.0/alloc_singleton/
//!
//! # Examples
//!
//! The most common way of using this pool is as a global singleton; the singleton mode gives you
//! automatic deallocation of memory blocks on `drop`.
//!
//! - `no_std`
//!
//! ``` ignore
//! #![no_main]
//! #![no_std]
//!
//! use lifo::{pool, singleton::Box};
//!
//! // instantiate a memory pool of `[u8; 128]` blocks as a global singleton
//! pool!(A: [u8; 128]);
//!
//! #[entry]
//! fn main() -> ! {
//!     static mut MEMORY: [u8; 1024] = [0; 1024];
//!
//!     // increase the capacity of the pool by ~8 blocks
//!     A::grow(MEMORY);
//!
//!     // claim a block of memory
//!     // note that the type is `Box<A>`, and not `Box<[u8; 128]>`
//!     // `A` is the "name" of the pool
//!     let x: Box<A, _> = A::alloc().unwrap();
//!     loop {
//!         // .. do stuff with `x` ..
//!     }
//! }
//!
//! #[exception]
//! fn SysTick() {
//!     // claim a block of memory
//!     let y = A::alloc().unwrap();
//!
//!     // .. do stuff with `y` ..
//!
//!     // return the memory block to the pool
//!     drop(y);
//! }
//! ```
//!
//! - `std` -- note that, when using `std`, `Pool` does *not* implement `Sync` and `pool!` is not
//! available
//!
//! ```
//! use lifo::Pool;
//!
//! let pool = Pool::<[u8; 128]>::new();
//!
//! pool.grow(Box::leak(Box::new([0; 1024])));
//!
//! let x = pool.alloc().unwrap();
//!
//! // .. do stuff with `x` ..
//!
//! // return the memory to the pool
//! pool.free(x);
//! ```
//!
//! # Performance
//!
//! Measured on a ARM Cortex-M3 core running at 8 MHz and with zero Flash wait cycles
//!
//! N | `alloc` (`3`) | `alloc` (`z`) | `alloc` (`asm`) | `free` (`3`) | `free` (`z`) | `free` (`asm`)
//! --|---------------|---------------|-----------------|--------------|--------------|---------------
//! 0 | 19            | 22            | 11              | 18           | 16           | 9
//! 1 | 44            | 48            | 18              | 40           | 36           | 18
//! 2 | 68            | 72            | 26              | 43           | 45           | 20
//! 3 | 88            | 96            | 33              | 57           | 56           | 27
//! 4 | 113           | 120           | 45              | 68           | 67           | 37
//!
//! - `N` denotes the number of *interruptions*. On Cortex-M, an interruption consists of an
//!   interrupt handler preempting the would-be atomic section of the `alloc` / `free` operation.
//!   Note that it does *not* matter if the higher priority handler uses the same pool or not.
//! - All execution times are in clock cycles.
//! - Execution time is independent of `mem::size_of::<T>()`.
//! - The optimization level is indicated in parentheses. `asm` indicates that the "asm" feature is
//!   enabled.
//! - The numbers reported for `free` assume that `T` does *not* have a destructor.
//! - The numbers reported for `alloc` correspond to the successful path (i.e. `Some` variant is
//!   returned).
//! - Snippets used to benchmark the code:
//!
//! ``` ignore
//! #[inline(never)]
//! fn alloc() {
//!     asm::bkpt();
//!     let x = A::alloc();
//!     asm::bkpt();
//!     mem::forget(x);
//! }
//! ```
//!
//! ``` ignore
//! #[inline(never)]
//! fn free(x: Box<A, Uninit>) {
//!     asm::bkpt();
//!     drop(x);
//!     asm::bkpt();
//! }
//! ```
//! # Portability
//!
//! This pool internally uses a Treiber stack which is known to be susceptible to the ABA problem.
//! The only counter measure against the ABA problem that this library currently takes is relying on
//! LL/SC (Link-local / Store-conditional) instructions being used to implement CAS loops on the
//! target architecture (see section on ['Soundness'](#soundness) for more information). For this
//! reason, `Pool` only implements `Sync` when compiling for ARM Cortex-M.
//!
//! Also note that ARMv6-M lacks the primitives for CAS loops so this library will *not* compile for
//! `thumbv6m-none-eabi`.
//!
//! # MSRV
//!
//! This crate compiles on stable Rust 1.31.0 or newer.
//!
//! # Cargo features
//!
//! ## `arch`
//!
//! Replaces the internal implementation, which uses `AtomicPtr`, with an ARM architecture specific
//! implementation that use LL/SC primitives. This optimized implementation significantly reduces
//! the overhead of `alloc` and `free`, and also shrinks the "critical section" of `free` (`push`)
//! from 3 to 2 instructions. For reference, the critical section of `alloc` (`pop`) is 3
//! instructions for both implementations.
//!
//! ## `maybe-uninit`
//!
//! Enabling this features adds a `grow_exact` method to `Pool` and `singleton::Pool`. Like `grow`,
//! this method can be used increase the capacity of a pool; however, the given buffer will be fully
//! utilized.
//!
//! The argument of `grow_exact` is a static reference to `MaybeUninit`. As the `MaybeUninit` is
//! still unstable this feature requires using a nightly compiler.
//!
//! ## `union`
//!
//! Enabling this feature reduces the footprint of `Node`, making the pool more space efficient
//! (effectively zero cost). This feature depends on the unstable `untagged_unions` feature and thus
//! requires a nightly compiler.
//!
//! # Soundness
//!
//! This pool uses a Treiber stack to keep a list of free memory blocks (nodes). Each of these
//! nodes has a pointer to the next node. To claim a memory block we simply pop a node from the
//! top of the stack and use it as a memory block. The pop operation consists of swapping the
//! current head (top) node with the node below it. The Rust code for the `pop` operation is shown
//! below:
//!
//! ``` ignore
//! fn pop(&self) -> Option<NonNull<Node<T>>> {
//!     let fetch_order = ..;
//!     let set_order = ..;
//!
//!     // `self.head` has type `AtomicPtr<Node<T>>`
//!     let mut head = self.head.load(fetch_order);
//!     loop {
//!         if let Some(nn_head) = NonNull::new(head) {
//!             let next = unsafe { (*head).next };
//!
//!             // <~ preempted
//!
//!             match self
//!                 .head
//!                 .compare_exchange_weak(head, next, set_order, fetch_order)
//!             {
//!                 Ok(_) => break Some(nn_head),
//!                 // head was changed by some interrupt handler / thread
//!                 Err(new_head) => head = new_head,
//!             }
//!         } else {
//!             // stack is observed as empty
//!             break None;
//!         }
//!     }
//! }
//! ```
//!
//! In general, the `pop` operation is susceptible to the ABA problem. If this operation gets
//! preempted by some interrupt handler somewhere between the `head.load` and the
//! `compare_and_exchange_weak`, and that handler modifies the stack in such a way that the head
//! (top) of the stack remains unchanged then resuming the `pop` operation will corrupt the stack.
//!
//! An example: imagine we are doing on `pop` on stack that contains these nodes: `A -> B -> C`,
//! `A` is the head (top), `B` is next to `A` and `C` is next to `B`. The `pop` operation will do a
//! `CAS(&self.head, A, B)` operation to atomically change the head to `B` iff it currently is `A`.
//! Now, let's say a handler preempts the `pop` operation before the `CAS` operation starts and it
//! `pop`s the stack twice and then `push`es back the `A` node; now the state of the stack is `A ->
//! C`. When the original `pop` operation is resumed it will succeed in doing the `CAS` operation
//! setting `B` as the head of the stack. However, `B` was used by the handler as a memory block and
//! no longer is a valid free node. As a result the stack, and thus the allocator, is in a invalid
//! state.
//!
//! However, not all is lost because Cortex-M devices use LL/SC (Link-local / Store-conditional)
//! operations to implement CAS loops. Let's look at the actual disassembly of `pop`.
//!
//! ``` text
//! 08000130 <<lifo::Pool<T>>::pop>:
//!  8000130:       6802            ldr     r2, [r0, #0]
//!  8000132:       e00c            b.n     800014e <<lifo::Pool<T>>::pop+0x1e>
//!  8000134:       4611            mov     r1, r2
//!  8000136:       f8d2 c000       ldr.w   ip, [r2]
//!  800013a:       e850 2f00       ldrex   r2, [r0]
//!  800013e:       428a            cmp     r2, r1
//!  8000140:       d103            bne.n   800014a <<lifo::Pool<T>>::pop+0x1a>
//!  8000142:       e840 c300       strex   r3, ip, [r0]
//!  8000146:       b913            cbnz    r3, 800014e <<lifo::Pool<T>>::pop+0x1e>
//!  8000148:       e004            b.n     8000154 <<lifo::Pool<T>>::pop+0x24>
//!  800014a:       f3bf 8f2f       clrex
//!  800014e:       2a00            cmp     r2, #0
//!  8000150:       d1f0            bne.n   8000134 <<lifo::Pool<T>>::pop+0x4>
//!  8000152:       2100            movs    r1, #0
//!  8000154:       4608            mov     r0, r1
//!  8000156:       4770            bx      lr
//! ```
//!
//! LDREX ("load exclusive") is the LL instruction, and STREX ("store exclusive") is the SC
//! instruction (see [1](#references)). On the Cortex-M, STREX will always fail if the processor
//! takes an exception between it and its corresponding LDREX operation (see [2](#references)). If
//! STREX fails then the CAS loop is retried (see instruction @ `0x8000146`). On single core
//! systems, preemption is required to run into the ABA problem and on Cortex-M devices preemption
//! always involves taking an exception. Thus the underlying LL/SC operations prevent the ABA
//! problem on Cortex-M.
//!
//! # References
//!
//! 1. [Cortex-M3 Devices Generic User Guide (DUI 0552A)][0], Section 2.2.7 "Synchronization
//! primitives"
//!
//! [0]: http://infocenter.arm.com/help/topic/com.arm.doc.dui0552a/DUI0552A_cortex_m3_dgug.pdf
//!
//! 2. [ARMv7-M Architecture Reference Manual (DDI 0403E.b)][1], Section A3.4 "Synchronization and
//! semaphores"
//!
//! [1]: https://static.docs.arm.com/ddi0403/eb/DDI0403E_B_armv7m_arm.pdf

// TODO update uses of `Ordering` (check generated DMB instructions) to make this multi-core safe
// TODO check if this also works on ARMv7-R

#![cfg_attr(feature = "arch", feature(link_llvm_intrinsics))]
#![cfg_attr(feature = "maybe-uninit", feature(maybe_uninit))]
#![cfg_attr(feature = "union", allow(unions_with_drop_fields))]
#![cfg_attr(feature = "union", feature(untagged_unions))]
#![cfg_attr(not(test), no_std)]
#![deny(missing_docs)]
#![deny(warnings)]

#[cfg(feature = "maybe-uninit")]
use core::mem::MaybeUninit;
#[cfg(not(feature = "arch"))]
use core::sync::atomic::{AtomicPtr, Ordering};
use core::{
    any::TypeId,
    cell::UnsafeCell,
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
};

use as_slice::{AsMutSlice, AsSlice};

pub use crate::singleton::Pool as pool;

#[cfg(feature = "arch")]
mod arch;
pub mod singleton;
#[cfg(test)]
mod tests;

/// A lock-free memory pool
pub struct Pool<T> {
    // Our "free list" is actually a Treiber stack
    #[cfg(not(feature = "arch"))]
    head: AtomicPtr<Node<T>>,

    #[cfg(feature = "arch")]
    head: UnsafeCell<*mut Node<T>>,

    // Current implementation is unsound on architectures that don't have LL/SC semantics so this
    // struct is not `Sync` on those platforms
    #[cfg(not(feature = "arch"))]
    _not_send_or_sync: PhantomData<*const ()>,
}

// NOTE: Here we lie about `Pool` implementing `Sync` on x86_64. This is not true but it lets us
// test the `pool!` and `singleton::Pool` abstractions. We just have to be careful not to use the
// pool in a multi-threaded context
#[cfg(any(armv7m, test))]
unsafe impl<T> Sync for Pool<T> {}

unsafe impl<T> Send for Pool<T> {}

impl<T> Pool<T> {
    /// Creates a new empty pool
    pub const fn new() -> Self {
        Pool {
            #[cfg(not(feature = "arch"))]
            head: AtomicPtr::new(ptr::null_mut()),

            #[cfg(feature = "arch")]
            head: UnsafeCell::new(ptr::null_mut()),

            #[cfg(not(feature = "arch"))]
            _not_send_or_sync: PhantomData,
        }
    }

    /// Claims a memory block from the pool
    ///
    /// Returns `None` when the pool is observed as exhausted
    ///
    /// *NOTE:* This method does *not* have bounded execution time; i.e. it contains a CAS loop
    pub fn alloc(&self) -> Option<Box<T, Uninit>> {
        if let Some(node) = self.pop() {
            Some(Box {
                node,
                _state: PhantomData,
            })
        } else {
            None
        }
    }

    /// Returns a memory block to the pool
    ///
    /// *NOTE*: `T`'s destructor (if any) will run on `value` iff `S = Init`
    ///
    /// *NOTE:* This method does *not* have bounded execution time; i.e. it contains a CAS loop
    pub fn free<S>(&self, value: Box<T, S>)
    where
        S: 'static,
    {
        if TypeId::of::<S>() == TypeId::of::<Init>() {
            unsafe {
                ptr::drop_in_place(value.node.as_ref().data.get());
            }
        }

        self.push(value.node)
    }

    /// Increases the capacity of the pool
    ///
    /// This method might *not* fully utilize the given memory block due to alignment requirements
    pub fn grow(&self, memory: &'static mut [u8]) {
        let mut p = memory.as_mut_ptr();
        let mut len = memory.len();

        let align = mem::align_of::<Node<T>>();
        let sz = mem::size_of::<Node<T>>();

        #[cfg(test)]
        eprintln!("{:?} - {} - {}", p, align, sz);

        let rem = (p as usize) % align;
        if rem != 0 {
            let offset = align - rem;

            if offset >= len {
                // slice is too small
                return;
            }

            p = unsafe { p.add(offset) };
            len -= offset;
        }

        while len >= sz {
            self.push(unsafe { NonNull::new_unchecked(p as *mut _) });

            p = unsafe { p.add(sz) };
            len -= sz;
        }
    }

    /// Increases the capacity of the pool
    #[cfg(feature = "maybe-uninit")]
    pub fn grow_exact<A>(&self, memory: &'static mut MaybeUninit<A>)
    where
        A: AsMutSlice<Element = Node<T>>,
    {
        for p in unsafe { (*memory.as_mut_ptr()).as_mut_slice() } {
            self.push(NonNull::from(p))
        }
    }

    #[cfg(not(feature = "arch"))]
    fn pop(&self) -> Option<NonNull<Node<T>>> {
        // NOTE: currently we only support single core devices (i.e. Non-Shareable memory)
        let fetch_order = Ordering::Relaxed;
        let set_order = Ordering::Relaxed;

        let mut head = self.head.load(fetch_order);
        loop {
            if let Some(nn_head) = NonNull::new(head) {
                let next = unsafe { (*head).next };

                match self
                    .head
                    .compare_exchange_weak(head, next, set_order, fetch_order)
                {
                    Ok(_) => break Some(nn_head),
                    // head was changed by some interrupt handler
                    Err(new_head) => head = new_head,
                }
            } else {
                // stack is observed as empty
                break None;
            }
        }
    }

    #[cfg(feature = "arch")]
    fn pop(&self) -> Option<NonNull<Node<T>>> {
        use crate::arch;

        unsafe {
            loop {
                // State: Exclusive
                let head = arch::ldrex(self.head.get() as *const u32) as *mut Node<T>;

                if let Some(nn_head) = NonNull::new(head) {
                    let next = (*head).next;

                    if arch::strex(next as u32, self.head.get() as *mut u32) == 0 {
                        // State: Open
                        break Some(nn_head);
                    } else {
                        // some interrupt changed our state back to Open and STREX failed
                        continue;
                    }
                } else {
                    // stack is observed as empty
                    arch::clrex(); // State: Open
                    break None;
                }
            }
        }
    }

    #[cfg(not(feature = "arch"))]
    fn push(&self, mut new_head: NonNull<Node<T>>) {
        // NOTE: currently we only support single core devices (i.e. Non-Shareable memory)
        let fetch_order = Ordering::Relaxed;
        let set_order = Ordering::Relaxed;

        let mut head = self.head.load(fetch_order);
        loop {
            unsafe { new_head.as_mut().next = head }

            match self
                .head
                .compare_exchange_weak(head, new_head.as_ptr(), set_order, fetch_order)
            {
                Ok(_) => return,
                // head changed
                Err(p) => head = p,
            }
        }
    }

    #[cfg(feature = "arch")]
    fn push(&self, mut new_head: NonNull<Node<T>>) {
        unsafe {
            loop {
                // State: Exclusive
                let head = arch::ldrex(self.head.get() as *const u32) as *mut Node<T>;

                new_head.as_mut().next = head;

                if arch::strex(new_head.as_ptr() as u32, self.head.get() as *mut u32) == 0 {
                    // State: Open
                    break;
                } else {
                    // some interrupt changed our state back to Open and STREX failed
                    continue;
                }
            }
        }
    }
}

#[cfg(all(not(feature = "maybe-uninit"), not(feature = "union")))]
struct Node<T> {
    data: UnsafeCell<T>,
    next: *mut Node<T>,
}

/// Unfortunate implementation detail that you need to interact with if you want to use `grow_exact`
#[cfg(all(feature = "maybe-uninit", not(feature = "union")))]
pub struct Node<T> {
    data: UnsafeCell<T>,
    next: *mut Node<T>,
}

#[cfg(all(not(feature = "maybe-uninit"), feature = "union"))]
union Node<T> {
    data: UnsafeCell<T>,
    next: *mut Node<T>,
}

/// Unfortunate implementation detail that you need to interact with if you want to use `grow_exact`
#[cfg(all(feature = "maybe-uninit", feature = "union"))]
pub union Node<T> {
    data: UnsafeCell<T>,
    next: *mut Node<T>,
}

/// A memory block
pub struct Box<T, STATE = Init> {
    _state: PhantomData<STATE>,
    node: NonNull<Node<T>>,
}

impl<T> Box<T, Uninit> {
    /// Initializes this memory block
    pub fn init(self, val: T) -> Box<T, Init> {
        unsafe {
            ptr::write(self.node.as_ref().data.get(), val);
        }

        Box {
            node: self.node,
            _state: PhantomData,
        }
    }
}

/// Uninitialized type state
pub enum Uninit {}

/// Initialized type state
pub enum Init {}

unsafe impl<T, S> Send for Box<T, S> where T: Send {}

unsafe impl<T, S> Sync for Box<T, S> where T: Sync {}

impl<A> AsSlice for Box<A>
where
    A: AsSlice,
{
    type Element = A::Element;

    fn as_slice(&self) -> &[A::Element] {
        self.deref().as_slice()
    }
}

impl<A> AsMutSlice for Box<A>
where
    A: AsMutSlice,
{
    fn as_mut_slice(&mut self) -> &mut [A::Element] {
        self.deref_mut().as_mut_slice()
    }
}

impl<T> Deref for Box<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.node.as_ref().data.get() }
    }
}

impl<T> DerefMut for Box<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.node.as_ref().data.get() }
    }
}
