//! Local patch: guard empty native framebuffers before GL reads or row flipping.

pub(crate) fn has_area([width, height]: [u32; 2]) -> bool {
    width > 0 && height > 0
}

/// The reader supplies bottom-up RGBA bytes. Empty/overflowed sizes do not call it.
pub(crate) fn read_rgba(size: [u32; 2], read: impl FnOnce(&mut [u8])) -> Option<Vec<u8>> {
    if !has_area(size) {
        return None;
    }
    let row_bytes = (size[0] as usize).checked_mul(4)?;
    let mut pixels = vec![0; row_bytes.checked_mul(size[1] as usize)?];
    read(&mut pixels);
    // Reverse rows in place without a second framebuffer-sized allocation.
    for row in 0..size[1] as usize / 2 {
        let opposite = size[1] as usize - 1 - row;
        let (before, after) = pixels.split_at_mut(opposite * row_bytes);
        before[row * row_bytes..(row + 1) * row_bytes].swap_with_slice(&mut after[..row_bytes]);
    }
    Some(pixels)
}
