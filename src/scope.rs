use std::cell::Cell;
use std::marker::PhantomData;
use std::mem::transmute;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use crate::Aligned;

pub struct Scope<'scope> {
    pub(crate) state: &'scope State,
    _marker: PhantomData<*mut ()>,
}

impl Scope<'_> {
    pub fn broadcast<F>(&self, f: F)
    where
        F: Fn(usize) + Sync,
    {
        let state = self.state;

        let work = |thread, _stop: &mut bool| f(thread);

        // SAFETY: `_guard` will reset `state.work` before this function returns,
        // but only after all pending workers are finished.
        unsafe {
            state.work.set(transmute::<&Work, &'static Work>(&work));
        }

        state.pending.store(self.state.workers, Ordering::Relaxed);
        state.generation.fetch_add(1, Ordering::Release);

        struct ResetGuard<'scope>(&'scope State);

        impl Drop for ResetGuard<'_> {
            fn drop(&mut self) {
                let state = self.0;

                while state.pending.load(Ordering::Acquire) != 0 {
                    thread::yield_now();
                }

                state.work.set(STOP);
            }
        }

        let _guard = ResetGuard(state);

        f(0);
    }
}

pub fn scope<F, R>(parallelism: Option<NonZeroUsize>, f: F) -> R
where
    F: for<'scope> FnOnce(Scope<'scope>) -> R,
{
    let parallelism = parallelism
        .or_else(|| thread::available_parallelism().ok())
        .map_or(1, NonZeroUsize::get);

    let state = &State {
        workers: parallelism - 1,
        work: Cell::new(STOP),
        pending: Aligned(AtomicUsize::new(0)),
        generation: Aligned(AtomicUsize::new(0)),
    };

    thread::scope(|scope| {
        for thread in 1..parallelism {
            thread::Builder::new()
                .name(format!("fork-join-scope-worker-{thread}"))
                .spawn_scoped(scope, move || state.worker(thread))
                .unwrap();
        }

        struct StopGuard<'scope>(&'scope State);

        impl Drop for StopGuard<'_> {
            fn drop(&mut self) {
                let state = self.0;

                state.work.set(STOP);

                state.generation.fetch_add(1, Ordering::Release);
            }
        }

        let _guard = StopGuard(state);

        f(Scope {
            state,
            _marker: PhantomData,
        })
    })
}

#[repr(C, align(128))]
pub(crate) struct State {
    pub(crate) workers: usize,
    work: Cell<&'static Work<'static>>,
    pending: Aligned<AtomicUsize>,
    generation: Aligned<AtomicUsize>,
}

unsafe impl Send for State {}

unsafe impl Sync for State {}

impl State {
    fn worker(&self, thread: usize) {
        let mut last_generation = 0;

        loop {
            loop {
                let curr_generation = self.generation.load(Ordering::Acquire);

                if last_generation != curr_generation {
                    last_generation = curr_generation;
                    break;
                } else {
                    thread::yield_now();
                }
            }

            let mut stop = false;

            self.work.get()(thread, &mut stop);

            if stop {
                return;
            }

            self.pending.fetch_sub(1, Ordering::Release);
        }
    }
}

type Work<'work> = dyn Fn(usize, &mut bool) + 'work;

const STOP: &Work = &|_thread, stop: &mut bool| *stop = true;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_works() {
        let parallelism = NonZeroUsize::new(10).unwrap();

        let mut counts = (0..parallelism.get())
            .map(|_| AtomicUsize::new(0))
            .collect::<Vec<_>>();

        scope(Some(parallelism), |scope| {
            scope.broadcast(|thread| {
                counts[thread].fetch_add(1, Ordering::Relaxed);
            });
        });

        for count in &mut counts {
            assert_eq!(*count.get_mut(), 1);
        }
    }

    #[test]
    fn scope_is_neither_send_nor_sync() {
        trait Ambiguous<A> {
            fn ambiguous() {}
        }

        impl<T> Ambiguous<()> for T {}

        struct IsSend;

        impl<T> Ambiguous<IsSend> for T where T: Send {}

        struct IsSync;

        impl<T> Ambiguous<IsSync> for T where T: Sync {}

        let _ = <Scope as Ambiguous<_>>::ambiguous;
    }
}
