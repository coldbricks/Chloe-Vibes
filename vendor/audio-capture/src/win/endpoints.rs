//! Read-only Windows playback endpoint discovery. All COM allocations stay on
//! the calling thread and are released before returning owned Rust strings.

use std::{marker::PhantomData, ptr::null_mut, rc::Rc};

use winapi::{
    shared::{winerror::RPC_E_CHANGED_MODE, wtypes::VT_LPWSTR},
    um::{
        combaseapi::{CoCreateInstance, CoTaskMemFree, CoUninitialize, CLSCTX_ALL},
        coml2api::STGM_READ,
        functiondiscoverykeys_devpkey::PKEY_Device_FriendlyName,
        mmdeviceapi::{
            eConsole, eRender, IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator,
            MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
        },
        objbase::CoInitialize,
        propidl::{PropVariantClear, PROPVARIANT},
        propsys::IPropertyStore,
        unknwnbase::IUnknown,
    },
    Class, Interface,
};

use super::common::{winapi_result, WinError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderEndpoint {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

// COM initialization must be balanced even when it returns S_FALSE. An
// existing apartment with another threading model can be used without owning
// an extra initialization reference. Rc makes this guard thread-bound.
pub(super) struct ComApartment(bool, PhantomData<Rc<()>>);

impl ComApartment {
    pub(super) fn new() -> Result<Self, WinError> {
        let result = unsafe { CoInitialize(null_mut()) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self(false, PhantomData));
        }
        winapi_result(result)?;
        Ok(Self(true, PhantomData))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct ComPtr<T>(*mut T);

impl<T> ComPtr<T> {
    fn empty() -> Self {
        Self(null_mut())
    }
}

impl<T> Drop for ComPtr<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { (*(self.0 as *mut IUnknown)).Release() };
        }
    }
}

struct TaskString(*mut u16);

impl Drop for TaskString {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(self.0 as _) };
    }
}

struct PropertyValue(PROPVARIANT);

impl Drop for PropertyValue {
    fn drop(&mut self) {
        unsafe { PropVariantClear(&mut self.0) };
    }
}

// The caller supplies a Windows-owned, null-terminated UTF-16 string.
unsafe fn wide_string(value: *const u16) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *value.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(value, len))
}

pub(super) unsafe fn endpoint_id(device: *mut IMMDevice) -> Result<String, WinError> {
    let mut id = TaskString(null_mut());
    winapi_result((*device).GetId(&mut id.0))?;
    Ok(wide_string(id.0))
}

unsafe fn endpoint_name(device: *mut IMMDevice) -> Result<String, WinError> {
    let mut properties = ComPtr::<IPropertyStore>::empty();
    winapi_result((*device).OpenPropertyStore(STGM_READ, &mut properties.0))?;
    let mut value = PropertyValue(std::mem::zeroed());
    winapi_result((*properties.0).GetValue(&PKEY_Device_FriendlyName, &mut value.0))?;
    if u32::from(value.0.vt) == VT_LPWSTR {
        Ok(wide_string(*value.0.data.pwszVal()))
    } else {
        Ok(String::new())
    }
}

fn enumerator() -> Result<ComPtr<IMMDeviceEnumerator>, WinError> {
    let mut result = ComPtr::empty();
    winapi_result(unsafe {
        CoCreateInstance(
            &MMDeviceEnumerator::uuidof(),
            null_mut(),
            CLSCTX_ALL,
            &IMMDeviceEnumerator::uuidof(),
            &mut result.0 as *mut _ as _,
        )
    })?;
    Ok(result)
}

unsafe fn default_id(enumerator: *mut IMMDeviceEnumerator) -> Result<String, WinError> {
    let mut device = ComPtr::empty();
    winapi_result((*enumerator).GetDefaultAudioEndpoint(eRender, eConsole, &mut device.0))?;
    endpoint_id(device.0)
}

/// Current Windows default playback endpoint (the same eConsole role used by
/// `AudioCapture::init`). This does not activate or start an audio stream.
pub fn default_render_endpoint_id() -> Result<String, WinError> {
    let _apartment = ComApartment::new()?;
    let enumerator = enumerator()?;
    unsafe { default_id(enumerator.0) }
}

/// Active playback endpoints, with stable Windows IDs and friendly names.
/// A missing default does not prevent listing other active endpoints.
pub fn enumerate_render_endpoints() -> Result<Vec<RenderEndpoint>, WinError> {
    let _apartment = ComApartment::new()?;
    let enumerator = enumerator()?;
    let default = unsafe { default_id(enumerator.0) }.ok();
    let mut collection = ComPtr::<IMMDeviceCollection>::empty();
    winapi_result(unsafe {
        (*enumerator.0).EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &mut collection.0)
    })?;
    let mut count = 0;
    winapi_result(unsafe { (*collection.0).GetCount(&mut count) })?;
    let mut endpoints = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut device = ComPtr::empty();
        winapi_result(unsafe { (*collection.0).Item(index, &mut device.0) })?;
        let id = unsafe { endpoint_id(device.0) }?;
        let name = unsafe { endpoint_name(device.0) }
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| id.clone());
        endpoints.push(RenderEndpoint {
            is_default: default.as_ref() == Some(&id),
            id,
            name,
        });
    }
    endpoints.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(endpoints)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Read-only enumeration of this machine's active playback endpoints"]
    fn list_playback_endpoints() {
        // Exercise repeated calls on one apartment as well as fresh ownership.
        let _apartment = ComApartment::new().unwrap();
        for _ in 0..2 {
            let endpoints = enumerate_render_endpoints().unwrap();
            for endpoint in &endpoints {
                assert!(!endpoint.id.is_empty());
                assert!(!endpoint.name.is_empty());
                println!("{endpoint:?}");
            }
            println!("Default endpoint: {:?}", default_render_endpoint_id());
        }
    }
}
