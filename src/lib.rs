mod iter;
mod scope;

use std::ops::{Deref, DerefMut};

pub use scope::{Scope, scope};

#[derive(Clone, Copy, Default)]
#[repr(align(128))]
pub struct Aligned<T>(pub T);

impl<T> Deref for Aligned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Aligned<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone, Copy)]
struct Synced<T>(T);

unsafe impl<T> Send for Synced<T> {}

unsafe impl<T> Sync for Synced<T> {}
