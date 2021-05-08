//! `str` utilities

/// Find the byte offset of the first scalar value before `i` in a given byte
/// slice assumed to be a UTF-8 string. Returns `0` if there is no such
/// scalar value.
///
/// `i` must be on a scalar boundary.
pub fn utf8_str_prev(s: &[u8], mut i: usize) -> usize {
    debug_assert!(i <= s.len());

    // `i` must be on a scalar boundary
    debug_assert!(i >= s.len() || !is_utf8_continuation(s[i]));

    if i > 0 {
        while {
            i -= 1;
            i > 0 && is_utf8_continuation(s[i])
        } {}
    }
    i
}

fn is_utf8_continuation(x: u8) -> bool {
    (x as i8) < -0x40
}
