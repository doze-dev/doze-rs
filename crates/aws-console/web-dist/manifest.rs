// The placeholder bundle, written by hand until the console's build script (`bun run build` in console/apps/aws) generates
// this file. The generated one has the same shape: one entry per file, the .gz where there is one.

/// Every file of the bundle.
pub const FILES: &[(&str, Served)] = &[(
    "index.html",
    Served {
        content_type: "text/html; charset=utf-8",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/web-dist/index.html.gz"
        )),
        gzip: true,
    },
)];
