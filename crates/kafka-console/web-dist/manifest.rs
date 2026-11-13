// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/badge-BDAXQu6I.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/badge-BDAXQu6I.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/button-DHEUWbPm.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/button-DHEUWbPm.js.gz")),
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
        "assets/checkbox-94PEenSA.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/checkbox-94PEenSA.js.gz")),
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
        "assets/copy-button-B7DOeIiI.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/copy-button-B7DOeIiI.js.gz")),
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
        "assets/first-request-Ppte_gOa.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-Ppte_gOa.js.gz")),
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
        "assets/gallery-BnOqLlON.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-BnOqLlON.js.gz")),
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
        "assets/index-0IedK7T9.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-0IedK7T9.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-C9Fw4bv4.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-C9Fw4bv4.js.gz")),
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
        "assets/lag-RVC3wAAe.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-RVC3wAAe.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-RRWqPXPc.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-RRWqPXPc.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/list-DJOXKNNL.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-DJOXKNNL.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-CLV-ONFl.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-CLV-ONFl.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-jNffi2-s.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-jNffi2-s.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-BsU7EKso.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-BsU7EKso.js.gz")),
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
        "assets/pages-KQWYuuo5.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/pages-KQWYuuo5.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-CmAPE-9-.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-CmAPE-9-.js.gz")),
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
        "assets/radio-group-CEY60M8d.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-CEY60M8d.js.gz")),
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
        "assets/select-B_h4Zg-X.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/select-B_h4Zg-X.js.gz")),
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
        "assets/story-DqMVdNRm.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-DqMVdNRm.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/switch-GvROJPEN.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/switch-GvROJPEN.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-DrLGRdt9.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-DrLGRdt9.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tooltip-B4Zb9NSm.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tooltip-B4Zb9NSm.js.gz")),
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
        "assets/use-inert-page-D78MeCNt.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/use-inert-page-D78MeCNt.js.gz")),
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
        "assets/wire-ByQ6xtdw.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-ByQ6xtdw.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-dock-DnxgWLTJ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-DnxgWLTJ.js.gz")),
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
