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

#![feature(proc_macro_hygiene, decl_macro, result_map_or_else)]

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate rocket;
extern crate rocket_contrib;

extern crate rctlib;

use rocket::fairing::AdHoc;
use rocket::http::RawStr;
use rocket::http::Status;
use rocket::outcome::Outcome::{Failure, Success};
use rocket::request::FromFormValue;
use rocket::request::{self, FromRequest, Request};
use rocket::response::status::NotFound;
use rocket::response::Stream;
use rocket::State;
use rocket_contrib::json::Json;

use std::fs::File;
use std::io::prelude::*;
use std::io::{self, Read, SeekFrom};

use rctlib::*;

#[derive(Debug)]
struct AuthKey {
    pub auth_key: String,
}

#[derive(Debug)]
struct AuthKeyGuard {}

impl<'a, 'r> FromRequest<'a, 'r> for AuthKeyGuard {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> request::Outcome<AuthKeyGuard, ()> {
        let auth_key_state = request.guard::<State<AuthKey>>()?;
        if request.headers().get_one("auth_key") == Some(&auth_key_state.auth_key) {
            Success(AuthKeyGuard {})
        } else {
            Failure((Status::Unauthorized, ()))
        }
    }
}

struct VirtDiskReader {
    // Needed to make sure the virtual disk doesn't get detached until we are done
    _virt_disk: Box<VirtDisk>,
    ranges: Vec<VirtualDiskChangeRange>,
    reader: File,
    current_range_index: usize,
    bytes_read: u64,
}

impl<'a> VirtDiskReader {
    pub fn new(virt_disk: Box<VirtDisk>, ranges: Vec<VirtualDiskChangeRange>) -> VirtDiskReader {
        let path = virt_disk.get_physical_disk_path().unwrap();
        // Using BufReader would fail with the following error when reaching the end of the disk:
        // Os { code: 27, kind: Other, message: "The drive cannot find the sector requested." }.
        let mut reader = File::open(path).unwrap();

        if ranges.len() > 0 {
            let offset = ranges[0].offset;
            reader.seek(SeekFrom::Start(offset)).unwrap();
        }

        VirtDiskReader {
            _virt_disk: virt_disk,
            reader: reader,
            ranges: ranges,
            current_range_index: 0,
            bytes_read: 0,
        }
    }
}

impl Read for VirtDiskReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.ranges.len() == 0 {
            return Ok(0);
        }

        let length = loop {
            let range_length = self.ranges[self.current_range_index].length;
            let length = std::cmp::min(buf.len() as u64, range_length - self.bytes_read);

            if length == 0 && self.current_range_index + 1 < self.ranges.len() {
                self.current_range_index += 1;
                self.bytes_read = 0;
                let offset = self.ranges[self.current_range_index].offset;
                self.reader.seek(SeekFrom::Start(offset)).unwrap();
            } else {
                break length;
            }
        };

        let read = self.reader.read(&mut buf[0..length as usize])?;
        self.bytes_read += read as u64;
        Ok(read)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct VirtDiskInfo {
    pub virtual_size: u64,
    pub parent_path: Option<String>,
}

struct QueryStringRanges {
    ranges: Vec<VirtualDiskChangeRange>,
}

impl<'v> FromFormValue<'v> for QueryStringRanges {
    type Error = &'v RawStr;

    fn from_form_value(form_value: &'v RawStr) -> Result<QueryStringRanges, &'v RawStr> {
        let mut ranges: Vec<VirtualDiskChangeRange> = Vec::new();
        for s in form_value.split(",") {
            let v = s
                .split(":")
                .map(|x| x.parse::<u64>().unwrap())
                .collect::<Vec<_>>();
            if v.len() > 2 {
                return Err("Too many values separated by :".into());
            }
            ranges.push(VirtualDiskChangeRange {
                offset: v[0],
                length: v[1],
            });
        }
        Ok(QueryStringRanges { ranges: ranges })
    }
}

fn open_vdisk(path: &str, read_only: bool) -> Result<VirtDisk, NotFound<String>> {
    VirtDisk::open(&path, read_only).map_err(|e| match e.result() {
        ERROR_FILE_NOT_FOUND => NotFound(format!("Bad vdisk path: {}", path)),
        _ => panic!(e),
    })
}

#[get("/vdisk/<path>/info", format = "json")]
fn get_disk_info(path: String, _key: AuthKeyGuard) -> Result<Json<VirtDiskInfo>, NotFound<String>> {
    let vdisk = open_vdisk(&path, true)?;
    let virtual_size = vdisk.get_virtual_size().unwrap();
    let parent_path = vdisk.get_parent_path().map_or_else(
        |e| match e.result() {
            ERROR_VHD_INVALID_TYPE => None,
            _ => panic!(e),
        },
        |v| Some(v),
    );

    Ok(Json(VirtDiskInfo {
        virtual_size: virtual_size,
        parent_path: parent_path,
    }))
}

#[get("/vdisk/<path>/rct", format = "json")]
fn get_rct_info(path: String, _key: AuthKeyGuard) -> Result<Json<RCTInfo>, NotFound<String>> {
    let vdisk = open_vdisk(&path, true)?;
    let rct_info = vdisk.get_rct_info().unwrap();
    Ok(Json(rct_info))
}

#[put("/vdisk/<path>/rct?<enabled>")]
fn set_rct_info(path: String, enabled: bool, _key: AuthKeyGuard) -> Result<(), NotFound<String>> {
    let mut vdisk = open_vdisk(&path, false)?;
    vdisk.set_rct_info(enabled).unwrap();
    Ok(())
}

#[get("/vdisk/<path>/rct/<rct_id>/changes", format = "json")]
fn query_disk_changes(
    path: String,
    rct_id: String,
    _key: AuthKeyGuard,
) -> Result<Json<Vec<VirtualDiskChangeRange>>, NotFound<String>> {
    let vdisk = open_vdisk(&path, true)?;
    let disk_changes = vdisk.query_changes(&rct_id).map_err(|e| match e.result() {
        ERROR_VHD_MISSING_CHANGE_TRACKING_INFORMATION => {
            NotFound(format!("RCT ID not found: {}", rct_id))
        }
        _ => panic!(e),
    })?;
    Ok(Json(disk_changes))
}

#[get("/vdisk/<path>/content?<ranges>")]
fn get_disk_content(
    path: String,
    ranges: QueryStringRanges,
    _key: AuthKeyGuard,
) -> Result<io::Result<Stream<VirtDiskReader>>, NotFound<String>> {
    get_disk_content_common(path, ranges.ranges)
}

// Provide a POST alternative to GET due to the query string's length limits
#[post(
    "/vdisk/<path>/content",
    format = "application/json",
    data = "<ranges>"
)]
fn get_disk_content_post(
    path: String,
    ranges: Json<Vec<VirtualDiskChangeRange>>,
    _key: AuthKeyGuard,
) -> Result<io::Result<Stream<VirtDiskReader>>, NotFound<String>> {
    get_disk_content_common(path, ranges.to_vec())
}

fn get_disk_content_common(
    path: String,
    ranges: Vec<VirtualDiskChangeRange>,
) -> Result<io::Result<Stream<VirtDiskReader>>, NotFound<String>> {
    let vdisk = open_vdisk(&path, true)?;
    vdisk.attach().unwrap();
    let reader = VirtDiskReader::new(Box::new(vdisk), ranges);
    Ok(Ok(Stream::from(reader)))
}

fn main() {
    rocket::ignite()
        .attach(AdHoc::on_attach("auth_key", |rocket| {
            let auth_key = rocket
                .config()
                .get_string("auth_key")
                .expect("auth_key is a required config option");
            Ok(rocket.manage(AuthKey { auth_key: auth_key }))
        }))
        .mount(
            "/",
            routes![
                get_disk_info,
                get_rct_info,
                set_rct_info,
                query_disk_changes,
                get_disk_content,
                get_disk_content_post
            ],
        )
        .launch();
}
