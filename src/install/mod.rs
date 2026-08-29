//! Installing MariaDB Server through the platform's native mechanism.
//!
//! The two platforms have nothing in common beyond the shape of the workflow —
//! resolve a version, show a plan, verify, then apply — so they live apart:
//! [`repository`] configures the Linux distribution package repositories, and
//! [`msi`] drives the Windows installer package. Both keep the planning in pure
//! functions and confine the system changes to an `execute` method.

pub mod msi;
pub mod repository;

use anyhow::{bail, Result};

pub use msi::{MsiOptions, MsiPlan};
pub use repository::{AptFormat, Distro, Keyring, PackageManager, Plan, PlanOptions};

use crate::api::models::MajorReleasesRoot;

/// The rpm-style architecture of the running machine, which is also how the
/// downloads API spells the `cpu` of a release file.
pub fn current_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "i686",
        "powerpc64" => "ppc64le",
        other => other,
    }
}

/// Refuse early rather than failing halfway through with a permission error.
pub fn require_privileges() -> Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: geteuid is always safe to call and cannot fail.
        if unsafe { libc::geteuid() } != 0 {
            bail!("`install` writes to /etc and runs the package manager; re-run it as root");
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        if !is_elevated() {
            bail!(
                "`install` runs the MariaDB installer, which writes to Program Files and \
                 creates a service; re-run it from an elevated prompt"
            );
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    bail!("`install` is only supported on Linux and Windows")
}

/// Whether the current process token carries an elevated privilege set.
#[cfg(windows)]
fn is_elevated() -> bool {
    use std::mem::size_of;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: the token handle is opened and closed here, and the elevation
    // struct is passed with its own size, as GetTokenInformation requires.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );

        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Highest-numbered release with status `Stable`, used when no version is given.
pub fn latest_stable(releases: &MajorReleasesRoot) -> Option<&str> {
    releases
        .major_releases
        .iter()
        .filter(|release| release.release_status.eq_ignore_ascii_case("stable"))
        .max_by_key(|release| version_key(&release.release_id))
        .map(|release| release.release_id.as_str())
}

/// Compare release ids numerically: `10.11` is newer than `10.5`, which a
/// lexicographic comparison gets backwards.
fn version_key(release_id: &str) -> Vec<u32> {
    release_id
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Whether a release id names a point release (`12.3.3`) rather than a major
/// release (`12.3`). The two take different paths: a point release identifies an
/// installer directly, a major release has to be resolved to its newest point.
pub fn is_point_release(release_id: &str) -> bool {
    release_id.split('.').count() >= 3
}
