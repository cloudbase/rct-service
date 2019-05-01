To generate virtdisk.rs, use:

    bindgen "C:\Program Files (x86)\Windows Kits\10\Include\10.0.17704.0\um\virtdisk.h"  -o c:\dev\virtdisk.rs --whitelist-function OpenVirtualDisk --whitelist-function GetVirtualDiskInformation --whitelist-function SetVirtualDiskInformation --whitelist-function QueryChangesVirtualDisk --whitelist-function CloseHandle --whitelist-function AttachVirtualDisk --whitelist-function GetVirtualDiskPhysicalPath -- -include "C:\Program Files (x86)\Windows Kits\10\Include\10.0.17704.0\um\Windows.h"

Please note that some manual fixes are needed, most notably the proper
*stdcall* "calling convention and the link directives, e.g:

    #[link(name = "VirtDisk")]
    extern "stdcall" { ... }
