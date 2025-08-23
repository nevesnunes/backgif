//! Simple debug log wrapper.

macro_rules! debug {
    ($($args:expr),*) => {{
        if std::env::var("DEBUG").unwrap_or_default() == "1" {
            println!($($args),*);
        }
    }}
}
pub(crate) use debug;
