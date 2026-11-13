// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/access-p6lc4mwo.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/access-p6lc4mwo.js.gz")),
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
        "assets/bits-KsR_QT5Q.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/bits-KsR_QT5Q.js.gz")),
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
        "assets/combobox-H7nTHroe.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/combobox-H7nTHroe.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-dialogs-I_sfgS1t.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-dialogs-I_sfgS1t.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/create-no9V3osr.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/create-no9V3osr.js.gz")),
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
        "assets/dialogs-O_br2PdR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dialogs-O_br2PdR.js.gz")),
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
        "assets/dropdown-menu-D0vT9rlV.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dropdown-menu-D0vT9rlV.js.gz")),
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
        "assets/first-request-Bpt5zvaR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-Bpt5zvaR.js.gz")),
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
        "assets/gallery-CKZOTPuf.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-CKZOTPuf.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-CQVMBMQ4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-CQVMBMQ4.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-t-9qi2SI.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-t-9qi2SI.css.gz")),
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
        "assets/lab-DdleYB7V.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lab-DdleYB7V.js.gz")),
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
        "assets/lag-DIm_uhMF.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-DIm_uhMF.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-BKA4iTxB.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-BKA4iTxB.js.gz")),
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
        "assets/list-C72wSIzG.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-C72wSIzG.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-ldJXINQd.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-ldJXINQd.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-CNUaxzrJ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-CNUaxzrJ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-aG2MYDMH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-aG2MYDMH.js.gz")),
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
        "assets/pages-BOdh4M9_.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/pages-BOdh4M9_.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-TNsKvloj.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-TNsKvloj.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-lQvOLOSV.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-lQvOLOSV.js.gz")),
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
        "assets/radio-group-C6QmtRaj.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-C6QmtRaj.js.gz")),
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
        "assets/settings-Dt5feP7W.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/settings-Dt5feP7W.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-C1Q8rXeO.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-C1Q8rXeO.js.gz")),
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
        "assets/tabs-BH3UZj89.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tabs-BH3UZj89.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-CN7df3mB.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-CN7df3mB.js.gz")),
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
        "assets/wire-CT6TMztD.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-CT6TMztD.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-DIy94agW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-DIy94agW.js.gz")),
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
