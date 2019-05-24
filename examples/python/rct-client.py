#!/usr/bin/env python
# Copyright 2019 Cloudbase Solutions Srl
#
#    Licensed under the Apache License, Version 2.0 (the "License"); you may
#    not use this file except in compliance with the License. You may obtain
#    a copy of the License at
#
#         http://www.apache.org/licenses/LICENSE-2.0
#
#    Unless required by applicable law or agreed to in writing, software
#    distributed under the License is distributed on an "AS IS" BASIS, WITHOUT
#    WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the
#    License for the specific language governing permissions and limitations
#    under the License.

import argparse
import requests
from requests.packages.urllib3 import exceptions
import sys

DEFAULT_MAX_BYTES_PER_REQUEST = 20 * 1024 * 1024


def get_disk_info(base_url, auth_key, disk_path, verify):
    url = "%s/vdisk/%s/info" % (base_url, disk_path)
    r = requests.get(
        url, headers={"auth_key": auth_key}, verify=verify)
    r.raise_for_status()
    return r.json()


def get_rct_info(base_url, auth_key, disk_path, verify):
    url = "%s/vdisk/%s/rct" % (base_url, disk_path)
    r = requests.get(
        url, headers={"auth_key": auth_key}, verify=verify)
    r.raise_for_status()
    return r.json()


def set_rct_info(base_url, auth_key, disk_path, enabled, verify):
    url = "%s/vdisk/%s/rct?enabled=%s" % (
        base_url, disk_path, str(enabled).lower())
    r = requests.put(
        url, headers={"auth_key": auth_key}, verify=verify)
    r.raise_for_status()


def query_disk_changes(base_url, auth_key, disk_path, rct_id, verify):
    url = "%s/vdisk/%s/rct/%s/changes" % (base_url, disk_path, rct_id)
    r = requests.get(
        url, headers={"auth_key": auth_key}, verify=verify)
    r.raise_for_status()
    return r.json()


def get_disk_content(base_url, auth_key, disk_path, out_file, ranges, verify):
    if not ranges:
        return

    url = "%s/vdisk/%s/content" % (base_url, disk_path)
    with requests.post(
            url, headers={"auth_key": auth_key}, stream=True, json=ranges,
            verify=verify) as r:
        r.raise_for_status()

        current_range_index = 0
        range_bytes_written = 0
        current_range = ranges[current_range_index]
        length = current_range["length"]
        out_file.seek(current_range["offset"])

        for chunk in r.iter_content(chunk_size=8192):
            while True:
                # The loop is needed in case the chunk includes data from
                # multiple ranges
                if not chunk:
                    break
                else:
                    buf = chunk[0:length - range_bytes_written]
                    out_file.write(buf)
                    range_bytes_written += len(buf)

                    if (length == range_bytes_written and
                            current_range_index + 1 < len(ranges)):
                        current_range_index += 1
                        range_bytes_written = 0
                        current_range = ranges[current_range_index]
                        length = current_range["length"]
                        out_file.seek(current_range["offset"])

                    chunk = chunk[len(buf):]


def show_rct_info(base_url, auth_key, disk_path, verify):
    disk_info = get_disk_info(
        base_url, auth_key, disk_path, verify=verify)
    print("Virtual disk info: %s" % disk_info)
    rct_info = get_rct_info(base_url, auth_key, disk_path, verify=verify)
    print("RCT status: %s" % rct_info)


def enable_rct(base_url, auth_key, disk_path, enable_rct, verify):
    set_rct_info(
        base_url, auth_key, disk_path, enabled=enable_rct, verify=verify)
    rct_info = get_rct_info(base_url, auth_key, disk_path, verify=verify)
    print("New RCT status: %s" % rct_info)


def download_to_local_raw_disk(base_url, auth_key, disk_path, rct_id,
                               local_filename, max_bytes_per_request,
                               verify):
    disk_info = get_disk_info(
        base_url, auth_key, disk_path, verify=verify)
    print("Virtual disk info: %s" % disk_info)

    rct_info = get_rct_info(base_url, auth_key, disk_path, verify=verify)
    print("RCT status: %s" % rct_info)

    if not rct_info["enabled"]:
        raise Exception("RCT not enabled for this disk")

    rct_id = rct_id or rct_info["most_recent_id"]

    disk_changes = query_disk_changes(
        base_url, auth_key, disk_path, rct_id, verify=verify)
    print("Disk changes: %d" % len(disk_changes))
    print("Total bytes: %d" % sum(d["length"] for d in disk_changes))

    with open(local_filename, 'wb') as f:
        f.truncate(disk_info["virtual_size"])
        tot_size = 0
        ranges = []
        for i, disk_change in enumerate(disk_changes):
            print("Requesting disk data %d/%d. Offset: %d, length: %d" %
                  (i + 1, len(disk_changes), disk_change["offset"],
                   disk_change["length"]))

            # TODO(alexpilotti): split a range past the limit
            tot_size += disk_change["length"]
            ranges.append(disk_change)

            if (max_bytes_per_request == 0 or
                    tot_size >= max_bytes_per_request):
                get_disk_content(base_url, auth_key, disk_path, f, ranges,
                                 verify=verify)
                tot_size = 0
                ranges = []

        if ranges:
            get_disk_content(base_url, auth_key, disk_path, f, ranges,
                             verify=verify)


def parse_arguments():
    def _get_long_type():
        if sys.version_info >= (3, 0):
            return int
        else:
            return long

    parser = argparse.ArgumentParser(
        description='Backup a Hyper-V virtual disk using RCT', add_help=True)
    parser.add_argument(
        '--base-url', type=str, default="https://localhost:6677",
        help='Base RCT service URL')
    parser.add_argument(
        '--auth-key', type=str, required=True,
        help='Auth key for the RCT service')
    parser.add_argument(
        '--remote-vhd-path', type=str, required=True,
        help='Path of the Hyper-V virtual disk (VHD or VHDX)')
    parser.add_argument(
        '--cert-path', type=str,
        help="X509 server certificate to be verified")
    parser.add_argument(
        '--rct-id', type=str,
        help="RCT id, using the last available one if not provided")
    parser.add_argument(
        '--max-bytes-per-request', type=_get_long_type(),
        default=DEFAULT_MAX_BYTES_PER_REQUEST,
        help="Max virtual disk bytes requested at once, aggregating multiple "
             "disk ranges. Set to 0 to perform one request for each virtual "
             "disk range. Default value is: %d" %
             DEFAULT_MAX_BYTES_PER_REQUEST)

    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        '--show-rct-info', action='store_true',
        help='Show the current RCT info for this virtual disk')
    group.add_argument(
        '--enable-rct', action='store_true', dest="enable_rct",
        help='Enable RCT for this virtual disk')
    group.add_argument(
        '--disable-rct', action='store_false', dest="enable_rct",
        help='Disable RCT for this virtual disk')
    group.add_argument(
        '--local-disk-path', type=str, help='Local RAW disk path')

    args = parser.parse_args()
    return args


def main():
    requests.packages.urllib3.disable_warnings(
        exceptions.InsecureRequestWarning)
    requests.packages.urllib3.disable_warnings(
        exceptions.SubjectAltNameWarning)

    args = parse_arguments()
    verify = args.cert_path or False

    if args.local_disk_path:
        download_to_local_raw_disk(
            args.base_url, args.auth_key, args.remote_vhd_path, args.rct_id,
            args.local_disk_path, args.max_bytes_per_request, verify)
    elif args.show_rct_info:
        show_rct_info(
            args.base_url, args.auth_key, args.remote_vhd_path, verify)
    else:
        enable_rct(
            args.base_url, args.auth_key, args.remote_vhd_path,
            args.enable_rct, verify)


if __name__ == "__main__":
    main()
