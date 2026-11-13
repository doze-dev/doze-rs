// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/access-RG9H3aeT.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/access-RG9H3aeT.js.gz")),
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
        "assets/bits-D1akBLLx.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/bits-D1akBLLx.js.gz")),
            gzip: true,
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
        "assets/combobox-DNx9eTIF.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/combobox-DNx9eTIF.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-dialogs-B8oYXaWC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-dialogs-B8oYXaWC.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/create-jk_1hw_O.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/create-jk_1hw_O.js.gz")),
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
        "assets/dialogs-BeVlxSCq.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dialogs-BeVlxSCq.js.gz")),
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
        "assets/eye-CS5ak36J.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/eye-CS5ak36J.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/first-request-djx03y4E.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-djx03y4E.js.gz")),
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
        "assets/gallery-CEwYiEt5.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-CEwYiEt5.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/inbox-BxoKHp-t.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/inbox-BxoKHp-t.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-71JX_Lf4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-71JX_Lf4.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-DAfojZkz.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-DAfojZkz.css.gz")),
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
        "assets/lab-BmqUuU8A.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lab-BmqUuU8A.js.gz")),
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
        "assets/lag-DXedaHhf.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-DXedaHhf.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-DMDG1uHm.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-DMDG1uHm.js.gz")),
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
        "assets/list-BOnynUB-.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-BOnynUB-.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-BtRfSSek.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-BtRfSSek.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-DcgGbdwK.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-DcgGbdwK.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-7__wgi7R.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-7__wgi7R.js.gz")),
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
        "assets/pages-B7bul54T.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/pages-B7bul54T.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-C9IK6CAx.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-C9IK6CAx.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-RbtoRgM4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-RbtoRgM4.js.gz")),
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
        "assets/radio-group-CkTnoCC-.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-CkTnoCC-.js.gz")),
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
        "assets/settings-DTTJwqb7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/settings-DTTJwqb7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-9_xafBRt.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-9_xafBRt.js.gz")),
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
        "assets/tabs-CLju3jY0.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tabs-CLju3jY0.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-BXEVCaH2.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-BXEVCaH2.js.gz")),
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
        "assets/wire-BZoAvQdA.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-BZoAvQdA.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-Ciruco6L.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-Ciruco6L.js.gz")),
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
