//! PTY locale defaults — Docker/minimal images often ship POSIX only (accents → `_`).

pub fn utf8_locale_vars() -> [(&'static str, &'static str); 2] {
    [("LANG", "C.UTF-8"), ("LC_ALL", "C.UTF-8")]
}
