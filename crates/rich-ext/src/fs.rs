//! What a file listing shows beside a name: times in the local time zone,
//! and the names of a file's owner and group.
//!
//! Not a port of anything upstream: an rs-rich addition, for file managers
//! (intuiTUIve's Yazi rebuild shows mtimes and owners with it).
//!
//! - [`local_time`] and [`short_local_time`] format a [`SystemTime`] in the
//!   local time zone, with `strftime` codes. They need this crate's
//!   off-by-default `local-time` feature, which brings in `chrono` (its
//!   `clock`, which reads the zone through `iana-time-zone`).
//! - [`user_name`] and [`group_name`] look a uid or gid up in the system's
//!   user database (`getpwuid_r`, `getgrgid_r`; on Unix, with no extra
//!   dependency) and remember each answer; elsewhere they return `None`.
//!
//! ```
//! # #[cfg(unix)] {
//! use rich_ext::fs::user_name;
//!
//! assert_eq!(user_name(0).as_deref(), Some("root"));
//! # }
//! ```

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
#[cfg(feature = "local-time")]
use std::time::SystemTime;

/// `time` in the local time zone, formatted with `strftime` codes
/// (`chrono`'s: `%Y-%m-%d %H:%M`, `%b %e %R`, `%c`, …). A format with an
/// unknown code gives the format back as it was rather than panicking.
///
/// ```
/// use std::time::{Duration, SystemTime};
/// use rich_ext::fs::local_time;
///
/// let time = SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 365);
/// let year = local_time(time, "%Y");
/// assert!(year == "1970" || year == "1971"); // depending on the zone
/// ```
#[cfg(feature = "local-time")]
pub fn local_time(time: SystemTime, format: &str) -> String {
    use std::fmt::Write;
    let local: chrono::DateTime<chrono::Local> = time.into();
    let mut out = String::new();
    if write!(out, "{}", local.format(format)).is_err() {
        return format.to_string();
    }
    out
}

/// `time` as a file listing shows it, in the local time zone: `10/18
/// 14:05` (month, day and time) within the current year, `10/18  2024`
/// (month, day and year) before it, as `ls -l` and Yazi do.
#[cfg(feature = "local-time")]
pub fn short_local_time(time: SystemTime) -> String {
    short_time_at(time, SystemTime::now())
}

#[cfg(feature = "local-time")]
fn short_time_at(time: SystemTime, now: SystemTime) -> String {
    use chrono::Datelike;
    let local: chrono::DateTime<chrono::Local> = time.into();
    let today: chrono::DateTime<chrono::Local> = now.into();
    if local.year() == today.year() {
        local.format("%m/%d %H:%M").to_string()
    } else {
        local.format("%m/%d  %Y").to_string()
    }
}

/// Names already looked up, by id.
type Names = Mutex<HashMap<u32, Option<String>>>;

fn cached(
    cache: &'static OnceLock<Names>,
    id: u32,
    look_up: fn(u32) -> Option<String>,
) -> Option<String> {
    let cache = cache.get_or_init(Default::default);
    if let Some(name) = cache.lock().ok().and_then(|names| names.get(&id).cloned()) {
        return name;
    }
    let name = look_up(id);
    if let Ok(mut names) = cache.lock() {
        names.insert(id, name.clone());
    }
    name
}

/// The name of the user with `uid` (`root` for 0), from the system's user
/// database, or `None` when it has no such user, or off Unix. Each uid is
/// looked up once; a listing of many files by one owner costs one lookup.
pub fn user_name(uid: u32) -> Option<String> {
    static USERS: OnceLock<Names> = OnceLock::new();
    cached(&USERS, uid, sys::user_name)
}

/// The name of the group with `gid` (`root` or `wheel` for 0), or `None`
/// when there is no such group, or off Unix. Cached as [`user_name`].
pub fn group_name(gid: u32) -> Option<String> {
    static GROUPS: OnceLock<Names> = OnceLock::new();
    cached(&GROUPS, gid, sys::group_name)
}

#[cfg(unix)]
mod sys {
    use std::ffi::CStr;

    /// Call `get` (`getpwuid_r` or `getgrgid_r`) with a buffer that grows
    /// while it says `ERANGE`, and read the name `name` points at.
    #[allow(unsafe_code)]
    fn look_up<T>(
        get: impl Fn(*mut T, *mut libc::c_char, usize, *mut *mut T) -> libc::c_int,
        name: impl Fn(&T) -> *const libc::c_char,
    ) -> Option<String> {
        let mut size = 1024usize;
        loop {
            let mut record = std::mem::MaybeUninit::<T>::uninit();
            let mut buffer = vec![0 as libc::c_char; size];
            let mut found: *mut T = std::ptr::null_mut();
            let code = get(record.as_mut_ptr(), buffer.as_mut_ptr(), size, &mut found);
            if code == libc::ERANGE && size < 1 << 20 {
                size *= 4;
                continue;
            }
            // Found, it is `record` that was filled in; anything else is
            // read as not found rather than followed.
            if code != 0 || found != record.as_mut_ptr() {
                return None;
            }
            // SAFETY: the call succeeded and filled in `record`.
            let record = unsafe { record.assume_init_ref() };
            let pointer = name(record);
            if pointer.is_null() {
                return None;
            }
            // SAFETY: a non-null name points at a string in `buffer`,
            // alive here.
            let text = unsafe { CStr::from_ptr(pointer) };
            return Some(text.to_string_lossy().into_owned());
        }
    }

    #[allow(unsafe_code)]
    pub(super) fn user_name(uid: u32) -> Option<String> {
        look_up(
            // SAFETY: every pointer is valid for the call, and `size` is
            // `buffer`'s length.
            |record, buffer, size, found| unsafe {
                libc::getpwuid_r(uid as libc::uid_t, record, buffer, size, found)
            },
            |record: &libc::passwd| record.pw_name,
        )
    }

    #[allow(unsafe_code)]
    pub(super) fn group_name(gid: u32) -> Option<String> {
        look_up(
            // SAFETY: as `user_name`'s.
            |record, buffer, size, found| unsafe {
                libc::getgrgid_r(gid as libc::gid_t, record, buffer, size, found)
            },
            |record: &libc::group| record.gr_name,
        )
    }
}

#[cfg(not(unix))]
mod sys {
    pub(super) fn user_name(_uid: u32) -> Option<String> {
        None
    }

    pub(super) fn group_name(_gid: u32) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn names_from_the_user_database() {
        assert_eq!(user_name(0).as_deref(), Some("root"));
        // Twice: the second from the cache.
        assert_eq!(user_name(0).as_deref(), Some("root"));
        assert!(matches!(group_name(0).as_deref(), Some("root" | "wheel")));
        // An id no system hands out.
        assert_eq!(user_name(u32::MAX - 7), None);
        assert_eq!(group_name(u32::MAX - 7), None);
        // This process's own user has a name.
        assert!(user_name(current_uid()).is_some());
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn current_uid() -> u32 {
        // SAFETY: getuid cannot fail.
        unsafe { libc::getuid() }
    }

    #[cfg(not(unix))]
    #[test]
    fn no_names_off_unix() {
        assert_eq!(user_name(0), None);
        assert_eq!(group_name(0), None);
    }

    #[cfg(feature = "local-time")]
    #[test]
    fn local_times() {
        use std::time::Duration;
        let day = 86_400;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(day * 365 * 50 + day * 180);
        let earlier = now - Duration::from_secs(3600);
        let years_ago = now - Duration::from_secs(day * 800);
        let recent = short_time_at(earlier, now);
        assert_eq!(recent.len(), "10/18 14:05".len(), "{recent}");
        assert!(recent.contains(':'));
        let old = short_time_at(years_ago, now);
        assert_eq!(old.len(), "10/18  2024".len(), "{old}");
        assert!(old.ends_with("2017") || old.ends_with("2018"), "{old}");
        assert_eq!(local_time(now, "%Y"), "2020");
        // An unknown code: the format back, not a panic.
        assert_eq!(local_time(now, "%Q"), "%Q");
        let full = local_time(now, "%Y-%m-%d %H:%M:%S %z");
        assert_eq!(full.len(), "2020-06-28 12:00:00 +0000".len(), "{full}");
    }
}
