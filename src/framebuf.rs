//! Shared latest-frame buffer so GDI blit happens on the UI thread.

use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct FrameBuffer {
    inner: Arc<Mutex<Option<Frame>>>,
}

pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}

impl FrameBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, width: i32, height: i32, bgra: Vec<u8>) {
        if let Ok(mut slot) = self.inner.lock() {
            *slot = Some(Frame { width, height, bgra });
        }
    }

    pub fn take_latest(&self) -> Option<Frame> {
        self.inner.lock().ok().and_then(|mut slot| slot.take())
    }
}
