//! Batch export authority stays in the parent; workers only see private staging.
use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, OpenOptions};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

/// The existing directory object selected at acquisition, plus missing children.
/// A later rename does not transfer authority to the replacement pathname.
pub(super) struct OutputRoot {
    dir: Dir,
    missing: PathBuf,
    path: PathBuf,
}

impl OutputRoot {
    pub fn capture(path: &Path) -> io::Result<Self> {
        let path = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        };
        let path = std::path::absolute(path)?;
        let mut ancestor = path.clone();
        let mut missing = Vec::new();
        loop {
            match std::fs::symlink_metadata(&ancestor) {
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    missing.push(
                        ancestor
                            .file_name()
                            .ok_or_else(|| io::Error::other("invalid output root"))?
                            .to_owned(),
                    );
                    if !ancestor.pop() {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        // This is the single ambient authority acquisition. All mutation below
        // is relative to retained directory handles, never this pathname.
        let dir = Dir::open_ambient_dir(&ancestor, cap_std::ambient_authority())?;
        Ok(Self {
            dir,
            missing: missing.into_iter().rev().collect(),
            path,
        })
    }

    pub fn destination(&self, path: &Path, create_parents: bool) -> io::Result<Destination> {
        let absolute = std::path::absolute(path)?;
        let relative = absolute
            .strip_prefix(&self.path)
            .map_err(|_| io::Error::other("destination is outside output root"))?;
        let mut components: Vec<OsString> = Vec::new();
        for component in self.missing.join(relative).components() {
            match component {
                Component::Normal(name) => components.push(name.to_owned()),
                Component::CurDir => (),
                _ => return Err(io::Error::other("output path contains traversal")),
            }
        }
        let leaf = components
            .pop()
            .ok_or_else(|| io::Error::other("missing output filename"))?;
        let mut dir = self.dir.try_clone()?;
        for name in components {
            if create_parents {
                match dir.create_dir(&name) {
                    Ok(()) => (),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                    Err(e) => return Err(e),
                }
            }
            // One component at a time: no intermediate symlink can be followed.
            dir = dir.open_dir_nofollow(&name)?;
        }
        Ok(Destination { dir, leaf })
    }
}

pub(super) struct Destination {
    dir: Dir,
    leaf: OsString,
}

impl Destination {
    pub fn publish(&self, source: &mut impl Read, overwrite: bool) -> io::Result<()> {
        self.publish_checked(source, overwrite, || {
            super::batch::check_interrupted()
                .map_err(|message| io::Error::new(io::ErrorKind::Interrupted, message))
        })
    }

    pub(super) fn publish_checked(
        &self,
        source: &mut impl Read,
        overwrite: bool,
        mut check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        check()?;
        if overwrite {
            // Never truncate a pre-existing inode (including an input hard link).
            let mut temporary = PrivateTemporary::new(&self.dir)?;
            copy_checked(source, temporary.file.as_mut().unwrap(), &mut check)?;
            check()?;
            temporary.replace(&self.leaf)
        } else {
            // The kernel enforces no-clobber at creation, including dangling links.
            let mut output = self
                .dir
                .open_with(&self.leaf, OpenOptions::new().write(true).create_new(true))?;
            copy_checked(source, &mut output, &mut check)
        }
    }
}

/// Bound work between cancellation checks; OS filesystem calls themselves may
/// still block. Never unlink a partial new file by name: it may have been swapped.
fn copy_checked(
    source: &mut impl Read,
    destination: &mut impl Write,
    check: &mut impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let mut buffer = [0; 64 * 1024];
    loop {
        check()?;
        let count = match source.read(&mut buffer) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        check()?;
        if count == 0 {
            return Ok(());
        }
        destination.write_all(&buffer[..count])?;
    }
}

/// Restrictive from creation, including platforms without anonymous tempfiles.
struct PrivateTemporary<'a> {
    dir: &'a Dir,
    name: OsString,
    file: Option<cap_std::fs::File>,
}

impl<'a> PrivateTemporary<'a> {
    fn new(dir: &'a Dir) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use cap_std::fs::OpenOptionsExt;
            // No other account can acquire a read handle before the protected
            // DACL is installed. WRITE_DAC is required by SetSecurityInfo.
            options.share_mode(0).access_mode(0x40000000 | 0x00040000);
        }
        #[cfg(not(any(unix, windows)))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "private replacement exports require Unix or Windows",
        ));
        for _ in 0..128 {
            let name = OsString::from(format!(".rich-export-{:032x}.tmp", fastrand::u128(..)));
            match dir.open_with(&name, &options) {
                Ok(file) => {
                    let temporary = Self {
                        dir,
                        name,
                        file: Some(file),
                    };
                    #[cfg(windows)]
                    restrict_windows_acl(temporary.file.as_ref().unwrap())?;
                    return Ok(temporary);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot reserve export temporary file",
        ))
    }

    fn replace(mut self, name: &std::ffi::OsStr) -> io::Result<()> {
        // Windows must release the exclusive file handle before renaming. Its
        // protected DACL now prevents other accounts from opening the contents.
        drop(self.file.take());
        self.dir.rename(&self.name, self.dir, name)?;
        self.name.clear();
        Ok(())
    }
}

impl Drop for PrivateTemporary<'_> {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.name.is_empty() {
            let _ = self.dir.remove_file(&self.name);
        }
    }
}

/// The only native FFI needed here: cap-std does not expose Windows file DACLs.
/// The newly created file remains exclusively held until its ACL is protected.
#[cfg(windows)]
#[allow(unsafe_code)]
fn restrict_windows_acl(file: &cap_std::fs::File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    // OW is the Owner Rights SID; P disables inherited grants.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)\0".encode_utf16().collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated UTF-16 and writable output pointer. The API
    // allocates descriptor; we free it exactly once after all derived pointers.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = std::ptr::null_mut();
        let result = if GetSecurityDescriptorDacl(
            descriptor,
            &mut present,
            &mut dacl,
            &mut defaulted,
        ) == 0
        {
            Err(io::Error::last_os_error())
        } else if present == 0 || dacl.is_null() {
            Err(io::Error::other("private export DACL missing"))
        } else {
            let code = SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            );
            if code == 0 {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(code as i32))
            }
        };
        LocalFree(descriptor);
        result
    }
}
