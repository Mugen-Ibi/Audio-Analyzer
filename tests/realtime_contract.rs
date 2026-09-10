use audio_analyzer::{
    capture,
    dsp::SpectrumAnalyzer,
    model::{AUDIO_BLOCK_FRAMES, AudioBlock, ChannelRoute, RuntimeStats},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::Arc,
};

struct CountingAllocator;
thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn record_allocation() {
    let _ = TRACKING.try_with(|tracking| {
        if tracking.get() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
    });
}

// The wrappers preserve the System allocator's layout and pointer contracts.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn callback_overload_and_fft_processing_allocate_nothing_after_construction() {
    let (mut writer, _input) = capture::channel(
        2,
        ChannelRoute::default_for_channels(2),
        Arc::new(RuntimeStats::default()),
    )
    .unwrap();
    let mut analyzer = SpectrumAnalyzer::new(48_000);
    let data = [0.25; AUDIO_BLOCK_FRAMES * 2];
    let mut block = AudioBlock {
        valid_frames: AUDIO_BLOCK_FRAMES,
        has_reference: true,
        ..AudioBlock::default()
    };
    ALLOCATIONS.set(0);
    TRACKING.set(true);
    for index in 0..128 {
        writer.write_interleaved(&data, |value| value);
        block.start_frame = index * AUDIO_BLOCK_FRAMES as u64;
        std::hint::black_box(analyzer.process_block(&block));
    }
    TRACKING.set(false);
    assert_eq!(ALLOCATIONS.get(), 0);
}
