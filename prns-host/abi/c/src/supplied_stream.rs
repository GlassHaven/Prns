//! Experimental host-contract extension: an interface whose byte stream the
//! application supplies as a connected descriptor, instead of naming an
//! address for the engine to dial. Declared in
//! `include/prns_host_experimental.h`, deliberately outside the generated
//! contract until the shape settles.

use std::ffi::c_void;
use std::time::Duration;

#[cfg(unix)]
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
#[cfg(unix)]
use std::sync::Arc;

use crate::{catch_status, status, PrnsIssuedCommand, PrnsStringView};
use prns_host_core::Status as AbiStatus;

/// Opens one connected, stream-oriented OS handle (a POSIX descriptor here;
/// a proper Windows shape would carry a SOCKET) and returns it, transferring
/// ownership to the engine. A negative return declines this attempt; the
/// engine waits out the respawn delay and calls again. Invoked from engine
/// worker threads — never the caller's thread — so it must be thread-safe,
/// may block while it dials, and must stay callable until the interface is
/// detached or the host released.
pub type SuppliedStreamOpenCallback = unsafe extern "C" fn(*mut c_void) -> i64;

#[cfg(unix)]
struct RegisteredOpener {
    callback: SuppliedStreamOpenCallback,
    context: *mut c_void,
}

// The header contract requires the callback and context to be thread-safe
// and to outlive the interface, mirroring RegisteredReadiness.
#[cfg(unix)]
unsafe impl Send for RegisteredOpener {}
#[cfg(unix)]
unsafe impl Sync for RegisteredOpener {}

#[cfg(unix)]
impl RegisteredOpener {
    fn open(&self) -> Result<OwnedFd, prns_host_native::SuppliedStreamDeclined> {
        let raw = unsafe { (self.callback)(self.context) };
        if raw < 0 || raw > i64::from(i32::MAX) {
            return Err(prns_host_native::SuppliedStreamDeclined { code: raw });
        }
        let owned = unsafe { OwnedFd::from_raw_fd(raw as RawFd) };
        match set_nonblocking(raw as RawFd) {
            Ok(()) => Ok(owned),
            Err(errno) => {
                drop(owned);
                Err(prns_host_native::SuppliedStreamDeclined {
                    code: i64::from(errno).saturating_neg(),
                })
            }
        }
    }
}

#[cfg(unix)]
fn set_nonblocking(fd: RawFd) -> Result<(), i32> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(last_errno());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(last_errno());
    }
    Ok(())
}

#[cfg(unix)]
fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn prns_host_attach_supplied_stream(
    host: *mut crate::PrnsHost,
    open: Option<SuppliedStreamOpenCallback>,
    context: *mut c_void,
    name: PrnsStringView,
    respawn_delay_millis: u64,
    bitrate_kind: u32,
    bitrate_bps: u64,
    out_command: *mut *mut PrnsIssuedCommand,
) -> u32 {
    catch_status(|| {
        let Some(open) = open else {
            return status(AbiStatus::InvalidArgument);
        };
        let name = match unsafe { crate::read_string(name) } {
            Ok(name) => name.to_string(),
            Err(error) => return error,
        };
        let bitrate = match crate::parse_bitrate(bitrate_kind, bitrate_bps) {
            Ok(bitrate) => bitrate,
            Err(error) => return error,
        };
        attach(host, open, context, name, respawn_delay_millis, bitrate, out_command)
    })
}

#[cfg(unix)]
fn attach(
    host: *mut crate::PrnsHost,
    open: SuppliedStreamOpenCallback,
    context: *mut c_void,
    name: String,
    respawn_delay_millis: u64,
    bitrate: prns_host_core::Bitrate,
    out_command: *mut *mut PrnsIssuedCommand,
) -> u32 {
    let opener = RegisteredOpener {
        callback: open,
        context,
    };
    let attach = prns_host_native::SuppliedStreamAttach {
        name,
        open: Arc::new(move || opener.open()),
        respawn_delay: Duration::from_millis(respawn_delay_millis),
        bitrate,
    };
    unsafe {
        crate::submit_command_with(host, out_command, |native, readiness| {
            native.submit_supplied_stream_attach(attach, Some(readiness))
        })
    }
}

#[cfg(not(unix))]
fn attach(
    _host: *mut crate::PrnsHost,
    _open: SuppliedStreamOpenCallback,
    _context: *mut c_void,
    _name: String,
    _respawn_delay_millis: u64,
    _bitrate: prns_host_core::Bitrate,
    _out_command: *mut *mut PrnsIssuedCommand,
) -> u32 {
    status(AbiStatus::Unsupported)
}
