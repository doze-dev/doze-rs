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
        "assets/first-request-CMZk8NdP.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-CMZk8NdP.js.gz")),
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
        "assets/gallery-DjS5vTt7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-DjS5vTt7.js.gz")),
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
        "assets/index-bGgHgBbl.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-bGgHgBbl.js.gz")),
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
        "assets/lag-CbHfnk8h.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-CbHfnk8h.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-DrcSOJMq.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-DrcSOJMq.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-B9dsH7La.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-B9dsH7La.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-DcilEhEW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-DcilEhEW.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-D0Ss4wj3.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-D0Ss4wj3.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-CFNKMCPJ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-CFNKMCPJ.js.gz")),
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
        "assets/parts-CwX2dNxx.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-CwX2dNxx.js.gz")),
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
        "assets/radio-group-CwNeSxkb.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-CwNeSxkb.js.gz")),
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
        "assets/story-BmYpiA3c.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-BmYpiA3c.js.gz")),
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
        "assets/tape-DEbl2EKm.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-DEbl2EKm.js.gz")),
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
        "assets/wire-bq_IwPlH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-bq_IwPlH.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-C5-PnAQ9.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-C5-PnAQ9.js.gz")),
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
