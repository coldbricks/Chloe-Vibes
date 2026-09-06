use core::fmt;
use std::ptr::null_mut;

use winapi::{
    shared::{
        guiddef,
        ksmedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, KSDATAFORMAT_SUBTYPE_PCM},
    },
    um::winbase::{
        FormatMessageA, LocalFree, FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM,
        FORMAT_MESSAGE_IGNORE_INSERTS,
    },
};

#[macro_export]
macro_rules! read_unaligned {
    ($v:ident $(. $field:ident)*) => {
        std::ptr::addr_of!((*$v) $(.$field)* ).read_unaligned()
    };
}

pub struct WinError(pub i32);

impl fmt::Debug for WinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WinError(id: {:x}, {})", self.0, error_to_string(self.0))
    }
}

impl fmt::Display for WinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", error_to_string(self.0))
    }
}

impl std::error::Error for WinError {}

#[track_caller]
pub fn winapi_result(hresult: i32) -> Result<(), WinError> {
    if hresult >= 0 {
        Ok(())
    } else {
        Err(WinError(hresult))
    }
}

fn error_to_string(code: i32) -> String {
    let mut buffer: *mut i8 = null_mut();
    unsafe {
        let size = FormatMessageA(
            FORMAT_MESSAGE_ALLOCATE_BUFFER
                | FORMAT_MESSAGE_FROM_SYSTEM
                | FORMAT_MESSAGE_IGNORE_INSERTS,
            null_mut(),
            code as u32,
            0,
            &mut buffer as *mut _ as *mut i8,
            0,
            null_mut(),
        );
        if buffer.is_null() || size == 0 {
            if !buffer.is_null() {
                LocalFree(buffer as _);
            }
            return format!("Windows error 0x{:08X}", code as u32);
        }
        let slice = std::slice::from_raw_parts(buffer as _, size as usize);
        let string = String::from_utf8_lossy(slice).trim().to_owned();
        LocalFree(buffer as _);
        string
    }
}

#[derive(PartialEq, Eq)]
pub struct Guid(u32, u16, u16, [u8; 8]);

impl Guid {
    pub const fn from_winapi(guid: guiddef::GUID) -> Self {
        Self(guid.Data1, guid.Data2, guid.Data3, guid.Data4)
    }
}

impl From<guiddef::GUID> for Guid {
    fn from(guid: guiddef::GUID) -> Self {
        Self::from_winapi(guid)
    }
}

pub const _AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x80000000;
pub const _AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x08000000;

pub const DATAFORMAT_SUBTYPE_PCM: Guid = Guid::from_winapi(KSDATAFORMAT_SUBTYPE_PCM);
pub const DATAFORMAT_SUBTYPE_IEEE_FLOAT: Guid = Guid::from_winapi(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_nonzero_hresult_is_accepted() {
        assert!(winapi_result(0).is_ok());
        assert!(winapi_result(1).is_ok());
        assert!(winapi_result(0x0889_0001).is_ok());
        assert!(winapi_result(0x8000_4005_u32 as i32).is_err());
    }

    #[test]
    fn unknown_windows_error_has_printable_fallback() {
        assert!(!format!("{}", WinError(0x6FFF_FFFF)).is_empty());
    }
}
