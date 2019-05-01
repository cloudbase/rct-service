## Hyper-V RCT Service

Hyper-V 2016 and subsequent releases include an improved virtual disk backup
API called **Resilent Change Tracking (RCT)** conceptually similar to
VMware's **Change Block Tracking (CBT)**.

RCT is based on both WMI and native API and while a [sample is available for
the former](https://github.com/MicrosoftDocs/Virtualization-Documentation/tree/live/hyperv-samples/taylorb-wmi/xhypervbackup)
, not much is available for understanding how the native RCT API can be used
except for the [reference documentation](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-querychangesvirtualdisk).

The native RCT function and structures are part of the Virtual Storage API
(virtdisk.h), in particular the new function
[QueryChangesVirtualDisk](https://docs.microsoft.com/en-us/windows/desktop/api/virtdisk/nf-virtdisk-querychangesvirtualdisk)
and extensions to the pre-existing
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
A fully functioning command line Python client is also included.

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

# Client

A Python client sample is included to showcase how to use the REST API:

    python rct-client.py --auth-key swordfish \
    --remote-vhd-path "C:\\VHDS\\mydisk.vhdx" \
    --local-disk-path mydisk.raw \
    --rct-id "rctX:5bfde23b:ce75:4303:b54f:6c18394f105c:00000001"

The RCT ID is optional, if not provided the last available one is used.
Local disk path contains the data obtained from the RCT service, in RAW format
(it can be converted to other formats with [qemu-img](https://cloudbase.it/qemu-img-windows/) if needed).