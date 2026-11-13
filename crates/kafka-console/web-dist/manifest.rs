// Written by web/scripts/build.ts (bun run build in web/). Do not edit.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[
    (
        "assets/access-CRuwyx9u.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/access-CRuwyx9u.js.gz")),
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
        "assets/bits-DoMDDa_h.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/bits-DoMDDa_h.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/bricolage-grotesque-latin-wght-normal-DLoelf7F.woff2",
        Served {
            content_type: "font/woff2",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/bricolage-grotesque-latin-wght-normal-DLoelf7F.woff2")),
            gzip: false,
        },
    ),
    (
        "assets/button-Ccj1oDx9.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/button-Ccj1oDx9.js.gz")),
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
        "assets/checkbox-jCFEAeGO.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/checkbox-jCFEAeGO.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/chevron-down-BJ7Ffma2.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/chevron-down-BJ7Ffma2.js.gz")),
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
        "assets/collapsible-DLXgzQl_.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/collapsible-DLXgzQl_.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/combobox-BKlQ4vt8.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/combobox-BKlQ4vt8.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/config-dialogs-BPdnKzzR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/config-dialogs-BPdnKzzR.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/create-DIds6uUR.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/create-DIds6uUR.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/data-table-D0Y2la9p.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/data-table-D0Y2la9p.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dialogs-BbfX3261.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dialogs-BbfX3261.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dist-B0E4R7wk.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dist-B0E4R7wk.js.gz")),
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
        "assets/doze-kafka-BWz9lWd5.svg",
        Served {
            content_type: "image/svg+xml",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/doze-kafka-BWz9lWd5.svg.gz")),
            gzip: true,
        },
    ),
    (
        "assets/dropdown-menu-Ckv1WePt.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/dropdown-menu-Ckv1WePt.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/eye-61j25jAL.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/eye-61j25jAL.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/first-request-Cmb4PTwf.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/first-request-Cmb4PTwf.js.gz")),
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
        "assets/gallery-lCdxXDQj.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/gallery-lCdxXDQj.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-BDSb_LiD.css",
        Served {
            content_type: "text/css; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-BDSb_LiD.css.gz")),
            gzip: true,
        },
    ),
    (
        "assets/index-CP3kcR8V.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/index-CP3kcR8V.js.gz")),
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
        "assets/lab-Ct-bKc_I.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lab-Ct-bKc_I.js.gz")),
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
        "assets/lag-Csfznl27.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/lag-Csfznl27.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/layout-BOFZ9mzU.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/layout-BOFZ9mzU.js.gz")),
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
        "assets/list-Dhoo5LT7.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/list-Dhoo5LT7.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/live-dE1NlhM1.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/live-dE1NlhM1.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/members-B4bZ07Ss.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/members-B4bZ07Ss.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/multi-select-CMA-uZ4F.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/multi-select-CMA-uZ4F.js.gz")),
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
        "assets/pages-CSkVrFIs.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/pages-CSkVrFIs.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-DjPHvdDb.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-DjPHvdDb.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/parts-VevSQtJZ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/parts-VevSQtJZ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/r4-DzRhUqk5.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/r4-DzRhUqk5.js.gz")),
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
        "assets/radio-group-CQrudKxp.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/radio-group-CQrudKxp.js.gz")),
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
        "assets/settings-CizylDDl.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/settings-CizylDDl.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/story-BF5nI54C.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/story-BF5nI54C.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/switch-BCIqwvIQ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/switch-BCIqwvIQ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tabs-ysVX2vhJ.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tabs-ysVX2vhJ.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/tape-CEFNqQZY.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/tape-CEFNqQZY.js.gz")),
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
        "assets/wire-dock-D0TmJbE6.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-dock-D0TmJbE6.js.gz")),
            gzip: true,
        },
    ),
    (
        "assets/wire-sAr4LoNN.js",
        Served {
            content_type: "text/javascript; charset=utf-8",
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/web-dist/assets/wire-sAr4LoNN.js.gz")),
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
