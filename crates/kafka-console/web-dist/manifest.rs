// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/badge-n87VD8mu.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/badge-n87VD8mu.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/button-z1FIQO04.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/button-z1FIQO04.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/check-C3fLBEgh.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/check-C3fLBEgh.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/checkbox-C5ZQ7o7C.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/checkbox-C5ZQ7o7C.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/chevron-left-q0W1-5EP.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/chevron-left-q0W1-5EP.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/circle-x-BYj92AIr.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/circle-x-BYj92AIr.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/data-table-pepAE5dH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/data-table-pepAE5dH.js.gz")),
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
        "assets/first-request-D3C3Bt-T.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-D3C3Bt-T.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/format-B3Nc0vLm.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/format-B3Nc0vLm.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/gallery-BhzbgN-Q.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-BhzbgN-Q.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-FiA88krE.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-FiA88krE.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-b6Z6h1eS.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-b6Z6h1eS.css.gz")),
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
        "assets/label-XzaojsHh.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/label-XzaojsHh.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/lag-Bg_CYP5u.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-Bg_CYP5u.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-BgVFL9V8.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-BgVFL9V8.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-CEOwJHhz.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-CEOwJHhz.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-SyTHjjnH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-SyTHjjnH.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-CA_Wq9cR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-CA_Wq9cR.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-_WnJTWgI.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-_WnJTWgI.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/overlay-COsoRcTI.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/overlay-COsoRcTI.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-3IpwyPru.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-3IpwyPru.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-CHfi3OXJ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-CHfi3OXJ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-group-B-3RidSX.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-B-3RidSX.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/select-BqEKuk6V.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/select-BqEKuk6V.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-BZcqk7o9.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-BZcqk7o9.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/switch-CV3vyXBK.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/switch-CV3vyXBK.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-wPHEvs79.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-wPHEvs79.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tooltip-DjsCydFW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tooltip-DjsCydFW.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-Cq02QsXf.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-Cq02QsXf.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-CSl4Dfwt.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-CSl4Dfwt.js.gz")),
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
