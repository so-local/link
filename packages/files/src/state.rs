//! Link-owned state in a process's home belongs below `.lnk`.

use std::path::{Path, PathBuf};

macro_rules! areas {
    ($($area:ident => $location:ident),+ $(,)?) => {
        /// A state location below a home's `.lnk` folder.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Area { $($area),+ }

        impl Area {
            /// Every area, from the same table the runtime uses.
            pub const ALL: &'static [Self] = &[$(Self::$area),+];

            /// Its path relative to the canonical `.lnk` folder.
            pub fn relative(self) -> &'static str {
                match self { $(Self::$area => crate::layout::Hidden::$location.relative()),+ }
            }
        }
    };
}

areas! {
    Context => Context,
    Configured => Configured,
    OpenclawDoctorRelease => OpenclawDoctorRelease,
    Jobs => Jobs,
    Measures => HarnessMeasureLog,
}

/// State in this explicit home, independent of `LNK_INSTALLATION_DIR`. A harness's
/// home is already isolated from the machine's settings by its caller.
pub fn path(home: &Path, area: Area) -> PathBuf {
    home.join(&crate::layout::LAYOUT.roots.hidden)
        .join(area.relative())
}
