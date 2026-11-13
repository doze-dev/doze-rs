// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/badge--s0HM-m9.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/badge--s0HM-m9.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/button-SVrB424p.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/button-SVrB424p.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/check-DFmfoezE.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/check-DFmfoezE.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/checkbox-BOZZLDlL.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/checkbox-BOZZLDlL.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/chevron-left-n6jkRLW4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/chevron-left-n6jkRLW4.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/circle-x-DIRqohsf.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/circle-x-DIRqohsf.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/data-table-pXV12ILa.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/data-table-pXV12ILa.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/first-request-DTp_irrj.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-DTp_irrj.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/format-DfJLQzi7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/format-DfJLQzi7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/gallery-BkjFmuIN.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-BkjFmuIN.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-BJMUgj3g.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-BJMUgj3g.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-z1R07AZo.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-z1R07AZo.js.gz")),
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
        "assets/label-DubuONPZ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/label-DubuONPZ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/lag-BDvcMdQa.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-BDvcMdQa.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-DqxQiER_.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-DqxQiER_.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-BUp5EbZz.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-BUp5EbZz.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-ByVLk4xo.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-ByVLk4xo.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-BUk7E6jX.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-BUk7E6jX.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-CU2Zxs25.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-CU2Zxs25.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/overlay-Cb0HGhJ0.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/overlay-Cb0HGhJ0.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-BG4O09EW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-BG4O09EW.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-NM_O7Emg.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-NM_O7Emg.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-group-C2i5CKRw.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-C2i5CKRw.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/select-BUOlD7iY.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/select-BUOlD7iY.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-35qNh-LV.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-35qNh-LV.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/switch-ByCSqbyL.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/switch-ByCSqbyL.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-BQjLepqW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-BQjLepqW.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tooltip-CxoTKUk6.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tooltip-CxoTKUk6.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-BQH2OuNV.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-BQH2OuNV.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-BRjMckG1.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-BRjMckG1.js.gz")),
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
