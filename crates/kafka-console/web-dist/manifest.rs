// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/collapsible-B9SktR6D.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/collapsible-B9SktR6D.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-dialogs-CQP9FBy_.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-dialogs-CQP9FBy_.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-route-FeufwNVs.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-route-FeufwNVs.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/copy-button-dxYxiuUH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/copy-button-dxYxiuUH.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/create-Bh4NQHs3.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/create-Bh4NQHs3.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dialogs-D7rvNT4b.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dialogs-D7rvNT4b.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dropdown-menu-CMqfyZ1n.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dropdown-menu-CMqfyZ1n.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/empty-state-BCDedYL7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/empty-state-BCDedYL7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/format-7i1yV4rc.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/format-7i1yV4rc.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/gallery-CWQX6vuR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-CWQX6vuR.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-B3heBXDm.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-B3heBXDm.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-ynWQ8A8o.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-ynWQ8A8o.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/input-BUJx8bmL.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/input-BUJx8bmL.js.gz")),
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
        "assets/label-DcHI0BWG.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/label-DcHI0BWG.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-BGoab6ZP.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-BGoab6ZP.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-group-BPNdASKP.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-BPNdASKP.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/select-0_bh_h_2.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/select-0_bh_h_2.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/spans-zAZhjloS.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/spans-zAZhjloS.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tabs-DoHqdxsy.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tabs-DoHqdxsy.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/textarea-CdvZfXIO.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/textarea-CdvZfXIO.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/toggle-group-C0POYT3h.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/toggle-group-C0POYT3h.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/useSearch-Dpxgl0ZC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/useSearch-Dpxgl0ZC.js.gz")),
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
