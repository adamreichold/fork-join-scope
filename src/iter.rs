use std::ops::Range;
use std::slice;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{Aligned, Synced, scope::Scope};

impl Scope<'_> {
    pub fn iter_static<F>(&self, work: Range<usize>, f: F)
    where
        F: Fn(usize, Range<usize>) + Sync,
    {
        let work_per_thread = work.len().div_ceil(self.state.workers + 1);

        self.broadcast(|thread| {
            let start = work.start + work_per_thread * thread;
            let end = work.end.min(start + work_per_thread);

            f(thread, start..end);
        });
    }

    pub fn for_each_static<T, F>(&self, work: &mut [T], f: F)
    where
        T: Send,
        F: Fn(&mut [T]) + Sync,
    {
        let work_ptr = Synced(work.as_mut_ptr());

        self.iter_static(0..work.len(), |_thread, range| {
            let work_ptr = work_ptr;

            let work =
                unsafe { slice::from_raw_parts_mut(work_ptr.0.add(range.start), range.len()) };

            f(work);
        });
    }

    pub fn fold_static<T, A, F>(&self, work: &[T], accum: &mut Vec<Aligned<A>>, f: F)
    where
        T: Send,
        A: Default + Send,
        F: Fn(&mut A, &[T]) + Sync,
    {
        accum.clear();
        accum.resize_with(self.state.workers + 1, Default::default);

        let work_ptr = Synced(work.as_ptr());
        let accum_ptr = Synced(accum.as_mut_ptr());

        self.iter_static(0..work.len(), |thread, range| {
            let work_ptr = work_ptr;
            let accum_ptr = accum_ptr;

            let work = unsafe { slice::from_raw_parts(work_ptr.0.add(range.start), range.len()) };
            let accum = unsafe { &mut *accum_ptr.0.add(thread) };

            f(accum, work);
        });
    }

    pub fn iter_dynamic<F>(&self, work: Range<usize>, f: F)
    where
        F: Fn(usize, usize) + Sync,
    {
        let next_index = AtomicUsize::new(work.start + self.state.workers + 1);

        self.broadcast(|thread| {
            let mut index = work.start + thread;

            loop {
                if index >= work.end {
                    return;
                }

                f(thread, index);

                index = next_index.fetch_add(1, Ordering::Relaxed);
            }
        });
    }

    pub fn for_each_dynamic<T, F>(&self, work: &mut [T], f: F)
    where
        T: Send,
        F: Fn(&mut T) + Sync,
    {
        let work_ptr = Synced(work.as_mut_ptr());

        self.iter_dynamic(0..work.len(), |_thread, index| {
            let work_ptr = work_ptr;

            let work = unsafe { &mut *work_ptr.0.add(index) };

            f(work);
        });
    }

    pub fn fold_dynamic<T, A, F>(&self, work: &[T], accum: &mut Vec<Aligned<A>>, f: F)
    where
        T: Send,
        A: Default + Send,
        F: Fn(&mut A, &T) + Sync,
    {
        accum.clear();
        accum.resize_with(self.state.workers + 1, Default::default);

        let work_ptr = Synced(work.as_ptr());
        let accum_ptr = Synced(accum.as_mut_ptr());

        self.iter_dynamic(0..work.len(), |thread, index| {
            let work_ptr = work_ptr;
            let accum_ptr = accum_ptr;

            let work = unsafe { &*work_ptr.0.add(index) };
            let accum = unsafe { &mut *accum_ptr.0.add(thread) };

            f(accum, work);
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::scope::scope;

    #[test]
    fn for_each_static_works() {
        let length = 1_000;

        let mut counts = (0..length).map(|_| 0).collect::<Vec<_>>();

        scope(None, |scope| {
            scope.for_each_static(&mut counts, |counts| {
                for count in counts {
                    *count += 1;
                }
            });
        });

        for count in counts {
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn fold_static_works() {
        let length = 1_000;

        let nums = (0..length).map(|index| index).collect::<Vec<_>>();
        let mut sums = Vec::new();

        scope(None, |scope| {
            scope.fold_static(&nums, &mut sums, |sum: &mut usize, nums| {
                for num in nums {
                    *sum += num;
                }
            });
        });

        let sum: usize = sums.into_iter().map(|sum| sum.0).sum();

        assert_eq!(sum, length * (length - 1) / 2);
    }

    #[test]
    fn for_each_dynamic_works() {
        let length = 1_000;

        let mut counts = (0..length).map(|_| 0).collect::<Vec<_>>();

        scope(None, |scope| {
            scope.for_each_dynamic(&mut counts, |count| {
                *count += 1;
            });
        });

        for count in counts {
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn fold_dynamic_works() {
        let length = 1_000;

        let nums = (0..length).map(|index| index).collect::<Vec<_>>();
        let mut sums = Vec::new();

        scope(None, |scope| {
            scope.fold_dynamic(&nums, &mut sums, |sum: &mut usize, num| {
                *sum += num;
            });
        });

        let sum: usize = sums.into_iter().map(|sum| sum.0).sum();

        assert_eq!(sum, length * (length - 1) / 2);
    }
}
