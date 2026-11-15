// Weight probe: links every dependency the core plan picks and calls into each,
// so fat LTO keeps what a real doze would keep. Throwaway; numbers go to weight.md.
use clap::{Parser, Subcommand};
use std::sync::Arc;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: Option<String>,
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Up { svc: Vec<String>, #[arg(short)] f: bool, #[arg(long)] last: bool },
    Down,
    Status { #[arg(short)] w: bool, #[arg(long)] graph: bool },
    Logs { svc: String, #[arg(short)] f: bool, #[arg(long)] since: Option<String> },
    Hcl { src: String },
    Num { a: String, b: String },
    Re { pat: String, s: String },
    Get { url: String },
    Verify { key: String, sig: String, msg: String },
    Names { home: String },
    Module { prog: String },
    Tun,
    Yaml { s: String },
    Completion { shell: clap_complete::Shell },
}

struct NoDownloads;
impl doze_module_host::Downloads for NoDownloads {
    fn resolve_major<'a>(&'a self, _: &'a str, _: &'a str) -> doze_module_host::BoxFuture<'a, Result<String, doze_module_proto::msg::error::Error>> {
        Box::pin(async { Err(doze_module_proto::msg::error::Error::new(doze_module_proto::msg::error::ErrorKind::Unavailable, "no")) })
    }
    fn ensure<'a>(&'a self, _: &'a doze_module_proto::msg::run::Ensure) -> doze_module_host::BoxFuture<'a, Result<doze_module_proto::msg::run::Ensured, doze_module_proto::msg::error::Error>> {
        Box::pin(async { Err(doze_module_proto::msg::error::Error::new(doze_module_proto::msg::error::ErrorKind::Unavailable, "no")) })
    }
}

fn main() {
    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    match cli.cmd {
        Cmd::Hcl { src } => {
            let b: Result<hcl_edit::structure::Body, _> = src.parse();
            println!("{:?}", b.map(|b| b.to_string()));
        }
        Cmd::Num { a, b } => {
            let mut cc = astro_float::Consts::new().unwrap();
            let x = astro_float::BigFloat::parse(&a, astro_float::Radix::Dec, 512, astro_float::RoundingMode::ToEven, &mut cc);
            let y = astro_float::BigFloat::parse(&b, astro_float::Radix::Dec, 512, astro_float::RoundingMode::ToEven, &mut cc);
            let z = x.div(&y, 512, astro_float::RoundingMode::ToEven).pow(&y, 512, astro_float::RoundingMode::ToEven, &mut cc);
            println!("{}", z.format(astro_float::Radix::Dec, astro_float::RoundingMode::ToEven, &mut cc).unwrap());
        }
        Cmd::Re { pat, s } => println!("{:?}", regex::Regex::new(&pat).map(|r| r.captures(&s).map(|c| c.len()))),
        Cmd::Get { url } => {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let mut roots = rustls::RootCertStore::empty();
            for c in rustls_native_certs::load_native_certs().certs { let _ = roots.add(c); }
            let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
            println!("{:?} {}", agent.get(&url).call().map(|r| r.status()), roots.len());
        }
        Cmd::Verify { key, sig, msg } => {
            let k: [u8; 32] = key.as_bytes()[..32].try_into().unwrap();
            let s: [u8; 64] = sig.as_bytes()[..64].try_into().unwrap();
            let vk = ed25519_dalek::VerifyingKey::from_bytes(&k);
            use sha2::Digest;
            println!("{:?} {:?}", vk.map(|v| v.verify_strict(msg.as_bytes(), &ed25519_dalek::Signature::from_bytes(&s))), sha2::Sha256::digest(msg.as_bytes()));
        }
        Cmd::Names { home } => rt.block_on(async {
            let reg = doze_names::Registry::open(std::path::Path::new(&home), "probe");
            let mut r = doze_names::resolver::serve(doze_names::resolver::ResolverConfig::from_registry(&reg));
            let mut f = doze_names::front_door::serve(doze_names::front_door::FrontConfig::for_zone(&reg, std::path::Path::new(&home)));
            println!("{} {}", r.bound(std::time::Duration::from_secs(1)).await, f.bound(std::time::Duration::from_secs(1)).await);
            r.close().await;
            f.close().await;
        }),
        Cmd::Module { prog } => rt.block_on(async {
            let s = doze_module_host::Services { downloads: Arc::new(NoDownloads), backends: None, notices: None };
            match doze_module_host::launch("postgres", std::path::Path::new(&prog), s).await {
                Ok(m) => println!("{}", m.release),
                Err(e) => println!("{e:?}"),
            }
        }),
        Cmd::Tun => {
            let dev = tun_rs::DeviceBuilder::new().ipv4("10.0.0.1", 24, None).build_sync();
            println!("{:?}", dev.map(|d| d.name().ok()));
        }
        Cmd::Yaml { s } => println!("{:?}", yaml_rust2::YamlLoader::load_from_str(&s).map(|d| d.len())),
        Cmd::Completion { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(shell, &mut Cli::command(), "doze", &mut std::io::stdout());
        }
        Cmd::Up { .. } | Cmd::Down | Cmd::Status { .. } | Cmd::Logs { .. } => rt.block_on(async {
            let l = tokio::net::UnixListener::bind("/tmp/x.sock");
            let v = serde_json::json!({"ok": l.is_ok(), "json": cli.json, "config": cli.config});
            println!("{v}");
            let _ = tokio::process::Command::new("true").status().await;
            let _ = tokio::signal::ctrl_c;
        }),
    }
}
