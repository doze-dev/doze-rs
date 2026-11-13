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
        "assets/first-request-BiHp_ezB.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-BiHp_ezB.js.gz")),
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
        "assets/gallery-BLF9YVrW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-BLF9YVrW.js.gz")),
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
        "assets/index-Cv_kmGGB.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-Cv_kmGGB.js.gz")),
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
        "assets/lag-Q6LOEKsp.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-Q6LOEKsp.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-D4FmUc0j.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-D4FmUc0j.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-BBtDtkSC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-BBtDtkSC.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-Dz0HXqrC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-Dz0HXqrC.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-oN0HU-lo.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-oN0HU-lo.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-DKg7rt-N.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-DKg7rt-N.js.gz")),
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
        "assets/parts-mG6n2c9i.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-mG6n2c9i.js.gz")),
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
        "assets/radio-group-BDmIxPIW.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-BDmIxPIW.js.gz")),
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
        "assets/story-BuRu0huC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-BuRu0huC.js.gz")),
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
        "assets/tape-r4xkYtCC.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-r4xkYtCC.js.gz")),
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
        "assets/wire-DVDaH99X.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-DVDaH99X.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-B6rCIqur.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-B6rCIqur.js.gz")),
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
