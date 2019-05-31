## Hyper-V RCT Service

Hyper-V 2016 and subsequent releases include an improved virtual disk backup
API called **Resilent Change Tracking (RCT)** conceptually similar to
VMware's **Change Block Tracking (CBT)**.

RCT is based on both WMI and native API and while a [sample is available for
the former](https://github.com/MicrosoftDocs/Virtualization-Documentation/tree/live/hyperv-samples/taylorb-wmi/xhypervbackup)
, not much is available for understanding how the native RCT API can be used
except for the [reference documentation](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-querychangesvirtualdisk).

The native RCT functions and structures are part of the Virtual Storage API
(virtdisk.h), in particular
[QueryChangesVirtualDisk](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-querychangesvirtualdisk)
plus extensions to the pre-existing
[OpenVirtualDisk](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-openvirtualdisk),
[GetVirtualDiskInformation](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-getvirtualdiskinformation) and
[SetVirtualDiskInformation](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-setvirtualdiskinformation).

The typical usage pattern consists in performing incremental backups by
generating an RCT identifier [via WMI](https://github.com/MicrosoftDocs/Virtualization-Documentation/tree/live/hyperv-samples/taylorb-wmi/xhypervbackup)
and use that to obtain a list of changed areas in a disk. The data can be
subsequently obtained by attaching the VHD / VHDX disk and read the provided
areas from its [phisycal mount point path](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-getvirtualdiskphysicalpath).

This project includes a REST API service to obtain the RCT info from a given
virtual disk and stream the data remotely over an authenticated HTTPS channel.

## Client

A Python client and CLI is available
[here](https://github.com/cloudbase/python-rctclient).


## Build

    cargo +nightly build --release

## Configure

Generate X509 certificate and key:

    openssl req -newkey rsa:2048 -x509 -keyout key.pem \
    -out cert.pem -days 3650 -nodes -subj '/CN=localhost'

Modify *Rocket.toml* setting the *auth_key* used by clients to autheticate.

## Run

The executable is located in:

    target\release\rct-service.exe

For development purposes you can also just run it with:

    cargo +nightly run
