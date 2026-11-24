//! What a program's `main` names before it parses its command line (the
//! formats it owns, its spec kinds), each made only when first asked for:
//! most commands ask for none, and making one can be its whole schema.

use std::sync::OnceLock;

/// Things of type `T` a program names, each by the function that makes
/// it, made together the first time any is asked for.
pub(crate) struct Named<T> {
    given: OnceLock<Vec<fn() -> T>>,
    made: OnceLock<Vec<T>>,
}

impl<T> Named<T> {
    pub(crate) const fn new() -> Self {
        Self {
            given: OnceLock::new(),
            made: OnceLock::new(),
        }
    }

    /// Name them, once: a second naming is ignored, as the first stands.
    pub(crate) fn name(&self, makers: &[fn() -> T]) {
        let _ = self.given.set(makers.to_vec());
    }

    /// Them, made at the first asking; none where none were named.
    pub(crate) fn get(&self) -> &[T] {
        self.made.get_or_init(|| {
            self.given
                .get()
                .into_iter()
                .flatten()
                .map(|make| make())
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static MADE: AtomicUsize = AtomicUsize::new(0);

    fn one() -> u32 {
        MADE.fetch_add(1, Ordering::SeqCst);
        1
    }

    fn two() -> u32 {
        MADE.fetch_add(1, Ordering::SeqCst);
        2
    }

    /// Naming makes nothing; the first asking makes each once, and later
    /// askings make nothing again.
    #[test]
    fn what_a_program_names_is_made_once_when_first_asked_for() {
        let named = Named::new();
        named.name(&[one, two]);
        assert_eq!(MADE.load(Ordering::SeqCst), 0);
        assert_eq!(named.get(), [1, 2]);
        assert_eq!(named.get(), [1, 2]);
        assert_eq!(MADE.load(Ordering::SeqCst), 2);
    }
}
