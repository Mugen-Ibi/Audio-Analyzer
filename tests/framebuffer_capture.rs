// Compile the exact pure helper used by the patched renderer, also in core tests.
#[path = "../vendor/egui_glow/src/screen_capture.rs"]
mod screen_capture;

#[test]
fn empty_framebuffers_never_read_gl_or_split_zero_sized_rows() {
    for size in [[0, 0], [0, 480], [640, 0]] {
        assert!(!screen_capture::has_area(size));
        assert!(screen_capture::read_rgba(size, |_| panic!("must not read GL")).is_none());
    }
}

#[test]
fn rgba_rows_are_flipped_without_reordering_pixels_or_channels() {
    let source: Vec<u8> = (0..24).collect();
    let actual = screen_capture::read_rgba([2, 3], |dst| dst.copy_from_slice(&source)).unwrap();
    assert_eq!(
        actual,
        [&source[16..24], &source[8..16], &source[..8]].concat()
    );
    assert_eq!(
        screen_capture::read_rgba([1, 1], |dst| dst.copy_from_slice(&[1, 2, 3, 4])),
        Some(vec![1, 2, 3, 4])
    );
}

#[test]
fn overflowing_framebuffer_size_is_rejected_before_the_reader() {
    assert!(
        screen_capture::read_rgba([u32::MAX, u32::MAX], |_| panic!("must not read GL")).is_none()
    );
}
