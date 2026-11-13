// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/button-BXeSHsI2.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/button-BXeSHsI2.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/collapsible-Dd-vtI_i.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/collapsible-Dd-vtI_i.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-dialogs-DUND1pDy.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-dialogs-DUND1pDy.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-route-K_mB96cy.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-route-K_mB96cy.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/create-Dmj2OU2M.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/create-Dmj2OU2M.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dialogs-BabAjPtN.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dialogs-BabAjPtN.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-BFn9YIJH.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-BFn9YIJH.js.gz")),
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
        "assets/dropdown-menu-DvCMF6Re.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dropdown-menu-DvCMF6Re.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/first-request-CiK8OPsh.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-CiK8OPsh.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/format-DIEUk3VF.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/format-DIEUk3VF.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/gallery-xY44Ef2w.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-xY44Ef2w.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-BKitNsrM.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-BKitNsrM.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-Bl6XgOHp.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-Bl6XgOHp.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index.esm-CzMvEJgS.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index.esm-CzMvEJgS.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/input-DUZBtvZj.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/input-DUZBtvZj.js.gz")),
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
        "assets/label-BjFpdPmj.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/label-BjFpdPmj.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-FB5Qwmfp.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-FB5Qwmfp.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/matchContext-mvUho1nU.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/matchContext-mvUho1nU.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/network-Bw6XZ3Ao.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/network-Bw6XZ3Ao.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-CnFC1Nke.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-CnFC1Nke.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/radio-group-RlO0EdAd.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-RlO0EdAd.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/select-CjctoZgr.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/select-CjctoZgr.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/switch-GJHU3Oiu.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/switch-GJHU3Oiu.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tabs-5DmarvXx.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tabs-5DmarvXx.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-BV7kWpNw.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-BV7kWpNw.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/textarea-DSqQ63PN.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/textarea-DSqQ63PN.js.gz")),
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
