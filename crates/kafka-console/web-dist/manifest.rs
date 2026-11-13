// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/access-CSM8QjUs.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/access-CSM8QjUs.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/badge-BUM4o4s4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/badge-BUM4o4s4.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/bits-Czpe6_pg.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/bits-Czpe6_pg.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/bricolage-grotesque-latin-wght-normal-DLoelf7F.woff2",
        Served {
            content_type: "font/woff2",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/bricolage-grotesque-latin-wght-normal-DLoelf7F.woff2")),
            gzip: false,
        },
    ),
    (
        "assets/button-BwkGppaO.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/button-BwkGppaO.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/calendar-clock-Boa0MWh7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/calendar-clock-Boa0MWh7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/check-DNxGyAh7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/check-DNxGyAh7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/checkbox-GstCWK5W.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/checkbox-GstCWK5W.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/chevron-left-B5i-PzjH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/chevron-left-B5i-PzjH.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/circle-x-BzKkUTZz.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/circle-x-BzKkUTZz.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/collapsible-V6x3Txo8.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/collapsible-V6x3Txo8.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/combobox-D6h4skZu.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/combobox-D6h4skZu.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-dialogs-DDkByPzC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-dialogs-DDkByPzC.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/create-DqG4xZRc.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/create-DqG4xZRc.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/data-table-DpAoRaXO.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/data-table-DpAoRaXO.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dialogs-CXIBGgpb.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dialogs-CXIBGgpb.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-C-nOvmZR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-C-nOvmZR.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-CW9ylRut.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-CW9ylRut.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-ChtFpSOe.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-ChtFpSOe.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-CyXLZaiu.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-CyXLZaiu.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/doze-kafka-BWz9lWd5.svg",
        Served {
            content_type: "image/svg+xml",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/doze-kafka-BWz9lWd5.svg.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dropdown-menu-BunIoBAc.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dropdown-menu-BunIoBAc.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/eye-CS5ak36J.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/eye-CS5ak36J.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/first-request-DXJ6PFbI.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-DXJ6PFbI.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/form-CgSpRyA7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/form-CgSpRyA7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/format-Dkj2bmzH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/format-Dkj2bmzH.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/gallery-CwOxAQLa.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-CwOxAQLa.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-4fhTAZzE.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-4fhTAZzE.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-C0P148wG.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-C0P148wG.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index.esm-Uz4Ov3zc.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index.esm-Uz4Ov3zc.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/inter-latin-wght-normal-Dx4kXJAl.woff2",
        Served {
            content_type: "font/woff2",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/inter-latin-wght-normal-Dx4kXJAl.woff2")),
            gzip: false,
        },
    ),
    (
        "assets/jetbrains-mono-latin-400-normal-V6pRDFza.woff2",
        Served {
            content_type: "font/woff2",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/jetbrains-mono-latin-400-normal-V6pRDFza.woff2")),
            gzip: false,
        },
    ),
    (
        "assets/jetbrains-mono-latin-500-normal-BWZEU5yA.woff2",
        Served {
            content_type: "font/woff2",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/jetbrains-mono-latin-500-normal-BWZEU5yA.woff2")),
            gzip: false,
        },
    ),
    (
        "assets/lab-Knu-Geoi.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lab-Knu-Geoi.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/label-nbXK0pxn.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/label-nbXK0pxn.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/lag-DKmPvTGW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-DKmPvTGW.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-Bumn531e.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-Bumn531e.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/link-Ckxw_WQN.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/link-Ckxw_WQN.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-CBFXUgkS.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-CBFXUgkS.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-B4X035B0.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-B4X035B0.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-aGsdGYF6.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-aGsdGYF6.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-CoY73SL6.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-CoY73SL6.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/network-CUW2efF8.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/network-CUW2efF8.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/overlay-J-kANQc_.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/overlay-J-kANQc_.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/pages-eXBUPdGc.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/pages-eXBUPdGc.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-B_79gPCk.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-B_79gPCk.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-CYhfpAyi.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-CYhfpAyi.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-DuJ-ovSR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-DuJ-ovSR.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-group-B2rujjTa.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-B2rujjTa.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/schemas-DKMhhdBB.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/schemas-DKMhhdBB.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/send-DxXbl5Ip.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/send-DxXbl5Ip.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/settings-Cwo-ZY6v.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/settings-Cwo-ZY6v.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-dO1xzLYQ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-dO1xzLYQ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/switch-DBOL4_vY.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/switch-DBOL4_vY.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tabs-CcTKt6y5.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tabs-CcTKt6y5.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-C9v_StyG.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-C9v_StyG.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/textarea-Dr5Ls0eZ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/textarea-Dr5Ls0eZ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/trash-B8CdR0UP.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/trash-B8CdR0UP.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-B78sTmOF.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-B78sTmOF.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-C5eszGHC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-C5eszGHC.js.gz")),
            gzip: true,
        },
    ),
    (
        "favicon.svg",
        Served {
            content_type: "image/svg+xml",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/favicon.svg.gz")),
            gzip: true,
        },
    ),
    (
        "index.html",
        Served {
            content_type: "text/html; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/index.html.gz")),
            gzip: true,
        },
    ),
];
