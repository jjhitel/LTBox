//! `ltbox-core` — domain layer shared across LTBox crates.
//!
//! AES-CBC `.x` decryption, GitHub client, i18n, and
//! rawprogram XML parser. Every fallible API returns [`Result<T>`] /
//! [`LtboxError`]. Port of the non-UI parts of Python LTBox v2.x.

pub mod app_paths;
pub mod connectivity;
pub mod crypto;
pub mod downloader;
pub mod error;
pub mod github;
pub mod i18n;
pub mod install_source;
pub mod lenovo_info;
pub mod lenovo_ota;
pub mod lenovo_qfil;
pub mod live_sink;
pub mod model;
pub mod obf;
pub mod partition_lun;
pub mod runtime;
pub mod safe_path;
pub mod sahara_xml;

pub use error::{LtboxError, Result};

/// Echo a line to stdout, the in-process live sink, and the caller's
/// `&mut Vec<String>`. Don't re-extend the closure's Vec post-flow —
/// the sink already streamed every line.
#[macro_export]
macro_rules! live {
    ($log:expr, $($arg:tt)*) => {{
        let _line = format!($($arg)*);
        println!("{}", _line);
        $crate::live_sink::push(_line.clone());
        $log.push(_line);
    }};
}

/// Translate `$key` and substitute `{name}` placeholders from a list of
/// `name = value` pairs. Eliminates the chain of
/// `tr("k").replace("{a}", &x).replace("{b}", &y)…` repeated across
/// every live-log emit site.
///
/// Each `value` is converted via `Display` (`format!("{}", v)`) so the
/// caller can pass `&str`, `String`, integers, floats, or anything else
/// implementing `Display`. For pre-formatted floats (`"{x:.1}"`) just
/// pass `&format!("{x:.1}")`.
///
/// ```ignore
/// live!(
///     log,
///     "[Driver] {}",
///     tr_args!(
///         "live_driver_progress_pct",
///         name = display_name,
///         pct = format!("{pct:>3}"),
///         downloaded = format!("{dl_mb:.1}"),
///     )
/// );
/// ```
#[macro_export]
macro_rules! tr_args {
    ($key:expr $(, $name:ident = $val:expr)* $(,)?) => {{
        let mut __s = $crate::i18n::tr($key);
        $(
            __s = __s.replace(
                concat!("{", stringify!($name), "}"),
                &format!("{}", $val),
            );
        )*
        __s
    }};
}
