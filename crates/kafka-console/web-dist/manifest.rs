// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/badge-BUM4o4s4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/badge-BUM4o4s4.js.gz")),
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
        "assets/combobox-DZ4Br4fR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/combobox-DZ4Br4fR.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-dialogs-D7EXOvNL.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-dialogs-D7EXOvNL.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/create-vqVbNt4c.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/create-vqVbNt4c.js.gz")),
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
        "assets/dialogs-DLBSrgD3.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dialogs-DLBSrgD3.js.gz")),
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
        "assets/first-request-BBL4GCzT.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-BBL4GCzT.js.gz")),
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
        "assets/gallery-DwmrLEI_.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-DwmrLEI_.js.gz")),
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
        "assets/index-6BEzEz0K.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-6BEzEz0K.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-CloAfc-V.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-CloAfc-V.js.gz")),
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
        "assets/label-nbXK0pxn.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/label-nbXK0pxn.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/lag-Bx8AGYrn.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-Bx8AGYrn.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-BgQ38-Mb.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-BgQ38-Mb.js.gz")),
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
        "assets/list-Cs15mkoP.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-Cs15mkoP.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-BldP4I_l.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-BldP4I_l.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-DmIqLRqf.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-DmIqLRqf.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-4iBdEHn0.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-4iBdEHn0.js.gz")),
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
        "assets/pages-JwZjnnjx.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/pages-JwZjnnjx.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-B3W7o9-M.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-B3W7o9-M.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-BewSNOlU.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-BewSNOlU.js.gz")),
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
        "assets/radio-group-CYq2sI8O.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-CYq2sI8O.js.gz")),
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
        "assets/story-B18UnXze.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-B18UnXze.js.gz")),
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
        "assets/tabs-Df3Y9bf4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tabs-Df3Y9bf4.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-ByHFUET3.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-ByHFUET3.js.gz")),
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
        "assets/trash-Cu57Z5mE.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/trash-Cu57Z5mE.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-BzluRJlQ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-BzluRJlQ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-Dsx6d_2j.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-Dsx6d_2j.js.gz")),
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
