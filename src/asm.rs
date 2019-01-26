#[inline(always)]
pub unsafe fn clrex() {
    asm!("CLREX");
}

#[inline(always)]
pub unsafe fn ldrex<T>(p: *const T) -> T {
    let r: T;
    // NOTE("volatile") reading `p` may return a different value on each load
    asm!("LDREX $0, [$1]" : "=r"(r) : "r"(p) : : "volatile");
    r
}

#[inline(always)]
pub unsafe fn strex<T>(p: *const T, v: T) -> bool {
    let r: u32;
    asm!("STREX $0, $1, [$2]" : "=r"(r) : "r"(v) "r"(p));
    r == 0
}
