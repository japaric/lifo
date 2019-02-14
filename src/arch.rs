//! Remove this in favor of `core::arch::arm::{clrex,ldrex,strex}`

extern "C" {
    #[link_name = "llvm.arm.clrex"]
    pub fn clrex();

    #[link_name = "llvm.arm.ldrex.p0i32"]
    pub fn ldrex(ptr: *const u32) -> u32;

    #[link_name = "llvm.arm.strex.p0i32"]
    pub fn strex(newval: u32, ptr: *mut u32) -> u32;
}
