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
        "assets/copy-button-Ca8fo0s-.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/copy-button-Ca8fo0s-.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/data-table-9STjSXib.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/data-table-9STjSXib.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-CZwrOln3.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-CZwrOln3.js.gz")),
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
        "assets/eye-B8CzIrFO.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/eye-B8CzIrFO.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/first-request-DUgLOu1C.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-DUgLOu1C.js.gz")),
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
        "assets/gallery-BPIXoUKD.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-BPIXoUKD.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/inbox-CuMpeE87.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/inbox-CuMpeE87.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-CaBbsj9J.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-CaBbsj9J.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-CfYgzycQ.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-CfYgzycQ.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/input-CLpMOteN.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/input-CLpMOteN.js.gz")),
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
        "assets/lag-COGAXm6a.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-COGAXm6a.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-BB1lR4OF.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-BB1lR4OF.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-yVhJCewj.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-yVhJCewj.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-uXt59srg.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-uXt59srg.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-Bfvv_BzV.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-Bfvv_BzV.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-8ZTRI3Y_.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-8ZTRI3Y_.js.gz")),
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
        "assets/pages-DPIvCAth.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/pages-DPIvCAth.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-DlAzqTjp.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-DlAzqTjp.js.gz")),
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
        "assets/radio-group-BCyDYCFG.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-BCyDYCFG.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/search-H41gJnEQ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/search-H41gJnEQ.js.gz")),
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
        "assets/send-D4Canxgc.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/send-D4Canxgc.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-C1vG9QYS.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-C1vG9QYS.js.gz")),
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
        "assets/tape-Cc7kAcl6.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-Cc7kAcl6.js.gz")),
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
        "assets/trash-KCjwuGn7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/trash-KCjwuGn7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/use-inert-page-Be5NEpU8.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/use-inert-page-Be5NEpU8.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/useNavigate-Cyqn2FVX.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/useNavigate-Cyqn2FVX.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-DjY1ou8E.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-DjY1ou8E.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-Drw9QXH9.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-Drw9QXH9.js.gz")),
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
