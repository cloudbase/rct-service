// Copyright 2019 Cloudbase Solutions Srl
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may
// not use this file except in compliance with the License. You may obtain
// a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS, WITHOUT
// WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the
// License for the specific language governing permissions and limitations
// under the License.

#[macro_use]
extern crate serde_derive;

mod virtdisk;

use std::ffi::{OsStr, OsString};
use std::os::windows::prelude::*;

use std::error::Error;
use std::fmt;

use virtdisk::*;

const VIRTUAL_STORAGE_TYPE_DEVICE_UNKNOWN: DWORD = 0;

const VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT: GUID = GUID {
    Data1: 0xec984aec,
    Data2: 0xa0f9,
    Data3: 0x47e9,
    Data4: [0x90, 0x1f, 0x71, 0x41, 0x5a, 0x66, 0x34, 0x5b],
};

const FALSE: BOOL = 0;
const TRUE: BOOL = 1;

const ERROR_SUCCESS: DWORD = 0;
const ERROR_INSUFFICIENT_BUFFER: DWORD = 122;

pub const ERROR_FILE_NOT_FOUND: DWORD = 2;
pub const ERROR_PATH_NOT_FOUND: DWORD = 3;
pub const ERROR_VHD_INVALID_TYPE: DWORD = 0xC03A001B;
pub const ERROR_VHD_MISSING_CHANGE_TRACKING_INFORMATION: DWORD = 0xC03A0030;

#[derive(Debug)]
pub struct VirtualDiskError {
    result: DWORD,
}

impl VirtualDiskError {
    pub fn new(result: DWORD) -> VirtualDiskError {
        VirtualDiskError { result: result }
    }

    pub fn result(&self) -> DWORD {
        self.result
    }
}

impl Error for VirtualDiskError {
    fn description(&self) -> &str {
        "RCT error"
    }
}

impl fmt::Display for VirtualDiskError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RCT error code: 0x{:X}", self.result)
    }
}

fn string_to_u16_vec(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0)) // Add NULL terminator
        .collect()
}

unsafe fn u16_ptr_to_string(ptr: *const u16) -> String {
    let len = (0..).take_while(|&i| *ptr.offset(i) != 0).count();
    let slice = std::slice::from_raw_parts(ptr, len);

    OsString::from_wide(slice).into_string().unwrap()
}

fn check_result(res: DWORD) -> Result<(), VirtualDiskError> {
    match res {
        ERROR_SUCCESS => Ok(()),
        _ => Err(VirtualDiskError::new(res)),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VirtualDiskChangeRange {
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RCTInfo {
    pub enabled: bool,
    pub newer_changes: bool,
    pub most_recent_id: String,
}

pub struct VirtDisk {
    vhd_handle: HANDLE,
}

impl VirtDisk {
    pub fn open(vhd_path: &str, read_only: bool) -> Result<VirtDisk, VirtualDiskError> {
        let vhd_path_u16 = string_to_u16_vec(vhd_path);

        let mut vst: VIRTUAL_STORAGE_TYPE = unsafe { std::mem::zeroed() };
        vst.DeviceId = VIRTUAL_STORAGE_TYPE_DEVICE_UNKNOWN;
        vst.VendorId = VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT;

        let mut op: _OPEN_VIRTUAL_DISK_PARAMETERS = unsafe { std::mem::zeroed() };
        op.Version = _OPEN_VIRTUAL_DISK_VERSION_OPEN_VIRTUAL_DISK_VERSION_3;
        unsafe { op.__bindgen_anon_1.Version3.ReadOnly = if read_only { TRUE } else { FALSE } };

        let mut vhd_handle: HANDLE = unsafe { std::mem::zeroed() };

        check_result(unsafe {
            OpenVirtualDisk(
                &mut vst,
                vhd_path_u16.as_ptr(),
                _VIRTUAL_DISK_ACCESS_MASK_VIRTUAL_DISK_ACCESS_NONE,
                _OPEN_VIRTUAL_DISK_FLAG_OPEN_VIRTUAL_DISK_FLAG_NONE,
                &mut op,
                &mut vhd_handle,
            )
        })?;
        Ok(VirtDisk {
            vhd_handle: vhd_handle,
        })
    }

    fn get_info(
        &self,
        version: GET_VIRTUAL_DISK_INFO_VERSION,
    ) -> Result<(Vec<u8>), VirtualDiskError> {
        let mut buf_size: DWORD = std::mem::size_of::<_GET_VIRTUAL_DISK_INFO>() as DWORD;

        loop {
            let mut buf: Vec<u8> = vec![0; buf_size as usize];

            let gvdi: &mut _GET_VIRTUAL_DISK_INFO =
                unsafe { &mut *(buf.as_mut_ptr() as *mut _ as *mut _GET_VIRTUAL_DISK_INFO) };
            gvdi.Version = version;

            let ret = unsafe {
                GetVirtualDiskInformation(
                    self.vhd_handle,
                    &mut buf_size,
                    gvdi,
                    std::ptr::null_mut(),
                )
            };

            if ret != ERROR_INSUFFICIENT_BUFFER {
                check_result(ret)?;
                return Ok(buf);
            }
        }
    }

    pub fn get_rct_info(&self) -> Result<RCTInfo, VirtualDiskError> {
        let buf = self
            .get_info(_GET_VIRTUAL_DISK_INFO_VERSION_GET_VIRTUAL_DISK_INFO_CHANGE_TRACKING_STATE)?;
        let gvdi: &_GET_VIRTUAL_DISK_INFO =
            unsafe { &*(buf.as_ptr() as *const _ as *const _GET_VIRTUAL_DISK_INFO) };
        let rct_enabled = unsafe { gvdi.__bindgen_anon_1.ChangeTrackingState.Enabled } != FALSE;
        let newer_changes =
            unsafe { gvdi.__bindgen_anon_1.ChangeTrackingState.NewerChanges } != FALSE;
        let most_recent_id = unsafe {
            u16_ptr_to_string(
                gvdi.__bindgen_anon_1
                    .ChangeTrackingState
                    .MostRecentId
                    .as_ptr(),
            )
        };

        Ok(RCTInfo {
            enabled: rct_enabled,
            newer_changes: newer_changes,
            most_recent_id: most_recent_id,
        })
    }

    pub fn set_rct_info(&mut self, enabled: bool) -> Result<(), VirtualDiskError> {
        let mut svdi: _SET_VIRTUAL_DISK_INFO = unsafe { std::mem::zeroed() };
        svdi.Version = _SET_VIRTUAL_DISK_INFO_VERSION_SET_VIRTUAL_DISK_INFO_CHANGE_TRACKING_STATE;
        svdi.__bindgen_anon_1.ChangeTrackingEnabled = if enabled { TRUE } else { FALSE };

        check_result(unsafe { SetVirtualDiskInformation(self.vhd_handle, &mut svdi) })
    }

    pub fn get_virtual_size(&self) -> Result<u64, VirtualDiskError> {
        let buf = self.get_info(_GET_VIRTUAL_DISK_INFO_VERSION_GET_VIRTUAL_DISK_INFO_SIZE)?;
        let gvdi: &_GET_VIRTUAL_DISK_INFO =
            unsafe { &*(buf.as_ptr() as *const _ as *const _GET_VIRTUAL_DISK_INFO) };
        let virtual_size = unsafe { gvdi.__bindgen_anon_1.Size.VirtualSize };
        Ok(virtual_size)
    }

    pub fn get_parent_path(&self) -> Result<String, VirtualDiskError> {
        let buf =
            self.get_info(_GET_VIRTUAL_DISK_INFO_VERSION_GET_VIRTUAL_DISK_INFO_PARENT_LOCATION)?;
        let gvdi: &_GET_VIRTUAL_DISK_INFO =
            unsafe { &*(buf.as_ptr() as *const _ as *const _GET_VIRTUAL_DISK_INFO) };
        Ok(unsafe {
            u16_ptr_to_string(
                gvdi.__bindgen_anon_1
                    .ParentLocation
                    .ParentLocationBuffer
                    .as_ptr(),
            )
        })
    }

    pub fn get_virtual_storage_type(&self) -> Result<u32, VirtualDiskError> {
        let buf = self
            .get_info(_GET_VIRTUAL_DISK_INFO_VERSION_GET_VIRTUAL_DISK_INFO_VIRTUAL_STORAGE_TYPE)?;
        let gvdi: &_GET_VIRTUAL_DISK_INFO =
            unsafe { &*(buf.as_ptr() as *const _ as *const _GET_VIRTUAL_DISK_INFO) };
        let virtual_storage_type = unsafe { gvdi.__bindgen_anon_1.VirtualStorageType.DeviceId };
        Ok(virtual_storage_type)
    }

    pub fn get_provider_sub_type(&self) -> Result<u32, VirtualDiskError> {
        let buf =
            self.get_info(_GET_VIRTUAL_DISK_INFO_VERSION_GET_VIRTUAL_DISK_INFO_PROVIDER_SUBTYPE)?;
        let gvdi: &_GET_VIRTUAL_DISK_INFO =
            unsafe { &*(buf.as_ptr() as *const _ as *const _GET_VIRTUAL_DISK_INFO) };
        let provider_sub_type = unsafe { gvdi.__bindgen_anon_1.ProviderSubtype };
        Ok(provider_sub_type)
    }

    pub fn query_changes(
        &self,
        change_tracking_id: &str,
    ) -> Result<Vec<VirtualDiskChangeRange>, VirtualDiskError> {
        let change_tracking_id_u16 = string_to_u16_vec(change_tracking_id);

        let mut ranges: Vec<VirtualDiskChangeRange> = Vec::new();

        let mut processed_length: ULONG64 = 0;
        let mut byte_offset: ULONG64 = 0;
        let virtual_size: ULONG64 = self.get_virtual_size()?;

        let buf: Vec<u8> = vec![0; std::mem::size_of::<_QUERY_CHANGES_VIRTUAL_DISK_RANGE>() * 100];
        let mut qcvd: Vec<_QUERY_CHANGES_VIRTUAL_DISK_RANGE> = unsafe { std::mem::transmute(buf) };
        unsafe {
            qcvd.set_len(qcvd.len() / std::mem::size_of::<_QUERY_CHANGES_VIRTUAL_DISK_RANGE>())
        };

        loop {
            let mut range_count: ULONG = qcvd.len() as ULONG;

            check_result(unsafe {
                QueryChangesVirtualDisk(
                    self.vhd_handle,
                    change_tracking_id_u16.as_ptr(),
                    byte_offset,
                    virtual_size - byte_offset,
                    _QUERY_CHANGES_VIRTUAL_DISK_FLAG_QUERY_CHANGES_VIRTUAL_DISK_FLAG_NONE,
                    qcvd.as_mut_ptr(),
                    &mut range_count,
                    &mut processed_length,
                )
            })?;

            for i in 0..range_count as usize {
                ranges.push(VirtualDiskChangeRange {
                    offset: qcvd[i].ByteOffset,
                    length: qcvd[i].ByteLength,
                });
            }

            if byte_offset + processed_length == virtual_size {
                return Ok(ranges);
            }

            byte_offset += processed_length;
        }
    }

    pub fn attach(&self) -> Result<(), VirtualDiskError> {
        let mut attach_parameters: _ATTACH_VIRTUAL_DISK_PARAMETERS = unsafe { std::mem::zeroed() };
        attach_parameters.Version = _ATTACH_VIRTUAL_DISK_VERSION_ATTACH_VIRTUAL_DISK_VERSION_1;

        check_result(unsafe {
            AttachVirtualDisk(
                self.vhd_handle,
                std::ptr::null_mut(),
                _ATTACH_VIRTUAL_DISK_FLAG_ATTACH_VIRTUAL_DISK_FLAG_READ_ONLY
                    | _ATTACH_VIRTUAL_DISK_FLAG_ATTACH_VIRTUAL_DISK_FLAG_NO_DRIVE_LETTER,
                0,
                &mut attach_parameters,
                std::ptr::null_mut(),
            )
        })?;
        Ok(())
    }

    pub fn get_physical_disk_path(&self) -> Result<String, VirtualDiskError> {
        let mut buf: Vec<u16> = vec![0u16; 1024];
        let mut buf_size: ULONG = (buf.len() * 2) as ULONG;

        check_result(unsafe {
            GetVirtualDiskPhysicalPath(self.vhd_handle, &mut buf_size, buf.as_mut_ptr())
        })?;
        Ok(unsafe { u16_ptr_to_string(buf.as_ptr()) })
    }
}

impl Drop for VirtDisk {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.vhd_handle) };
        self.vhd_handle = unsafe { std::mem::zeroed() };
    }
}
