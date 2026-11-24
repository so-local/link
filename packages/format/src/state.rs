//! States: a module's own enum, so this package knows no module's words.
//! [`states!`] gives each variant its word and its tone, once.
//!
//! [`states!`]: crate::states

use serde::Serialize;

/// How a state reads: the color its word takes on a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// As it should be, the outcome a person wanted: green.
    Good,
    /// As it should be, or on its way: plain.
    Quiet,
    /// Worth a look: yellow.
    Warn,
    /// Wrong: red.
    Bad,
}

/// A state with one word and one tone a variant.
pub trait State {
    /// The word, lowercase, the same on a screen and in `--json`.
    fn word(&self) -> &'static str;
    fn tone(&self) -> Tone;
}

/// A module's states:
///
/// ```text
/// link_format::states! {
///     pub enum BoxState {
///         Running("running") => Good,
///         Stopped("stopped") => Quiet,
///         NotAnswering("not answering") => Bad,
///     }
/// }
/// ```
#[macro_export]
macro_rules! states {
    ($(#[$m:meta])* $vis:vis enum $name:ident {
        $($(#[$vm:meta])* $variant:ident($word:literal) => $tone:ident),* $(,)?
    }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name { $($(#[$vm])* $variant),* }

        impl $crate::State for $name {
            fn word(&self) -> &'static str {
                match self { $($name::$variant => $word),* }
            }
            fn tone(&self) -> $crate::Tone {
                match self { $($name::$variant => $crate::Tone::$tone),* }
            }
        }

        impl From<$name> for $crate::Value {
            fn from(s: $name) -> Self {
                $crate::Value::State($crate::StateWord::of(&s))
            }
        }

        impl $name {
            /// Every state, for the test that each word is lowercase.
            #[allow(dead_code, reason = "read only by a module's test that each word is lowercase, so unused in its build")]
            pub const ALL: &'static [$name] = &[$($name::$variant),*];
        }
    };
}

/// Whether every word of `states` is lowercase words: what a module's
/// test of its states asserts.
pub fn words_are_lowercase<S: State>(states: &[S]) -> bool {
    states.iter().all(|s| {
        let w = s.word();
        !w.is_empty()
            && w.chars()
                .all(|c| c.is_lowercase() || c == ' ' || c == '-' || c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    crate::states! {
        enum BoxState {
            Running("running") => Good,
            NotAnswering("not answering") => Bad,
        }
    }

    #[test]
    fn a_state_has_its_word_and_tone() {
        use crate::State;
        assert_eq!(BoxState::NotAnswering.word(), "not answering");
        assert_eq!(BoxState::Running.tone(), crate::Tone::Good);
        assert!(super::words_are_lowercase(BoxState::ALL));
    }
}
