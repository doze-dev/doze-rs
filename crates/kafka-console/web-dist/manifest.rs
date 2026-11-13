// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/badge-DyRKcE0t.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/badge-DyRKcE0t.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/checkbox-YxYk7IKr.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/checkbox-YxYk7IKr.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/data-table-Dwp3bhz6.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/data-table-Dwp3bhz6.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-BrcXTY1U.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-BrcXTY1U.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/gallery-0kCFCScv.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-0kCFCScv.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-BGBzcYWx.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-BGBzcYWx.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-CeJyB1-c.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-CeJyB1-c.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/input-CHHVc96e.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/input-CHHVc96e.js.gz")),
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
        "assets/lag-CceOmLq1.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-CceOmLq1.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-C03hSVu7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-C03hSVu7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-BIiwFoFY.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-BIiwFoFY.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-DL4BLf1M.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-DL4BLf1M.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-DzFkGsun.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-DzFkGsun.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/overlay-CnMPM9EQ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/overlay-CnMPM9EQ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-_LJicPen.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-_LJicPen.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-group-BwuZkVZX.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-BwuZkVZX.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/rolldown-runtime-XKyaHtl9.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/rolldown-runtime-XKyaHtl9.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/select-pLYDfMAf.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/select-pLYDfMAf.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/skeleton-0RXbprAy.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/skeleton-0RXbprAy.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/stat-DMeC9OWp.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/stat-DMeC9OWp.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/status-dot-_aCN68ah.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/status-dot-_aCN68ah.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-BYFuhPbS.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-BYFuhPbS.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/use-inert-page-BfDaaUbo.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/use-inert-page-BfDaaUbo.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-t2rmHNmz.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-t2rmHNmz.js.gz")),
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
