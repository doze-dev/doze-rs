//! How one function process is launched: the command for its runtime, and the environment AWS gives it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Credentials a function's SDK finds in its environment.
#[derive(Clone, Debug)]
pub(crate) struct Creds {
    pub(crate) key: String,
    pub(crate) secret: String,
    pub(crate) token: String,
    /// When they stop working, in milliseconds since the epoch; 0 for ones that do not.
    pub(crate) expires_ms: i64,
}

/// Where each runtime family's executable is, when it is not the one on `PATH`.
#[derive(Clone, Debug, Default)]
pub struct Interpreters {
    /// `python3` replacement.
    pub python: Option<String>,
    /// `node` replacement.
    pub node: Option<String>,
    /// `ruby` replacement.
    pub ruby: Option<String>,
    /// `java` replacement.
    pub java: Option<String>,
    /// `dotnet` replacement.
    pub dotnet: Option<String>,
}

/// Everything needed to start the process for one function version.
#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) region: String,
    pub(crate) runtime: String,
    pub(crate) handler: String,
    /// The `Command` extension: the whole launch line.
    pub(crate) command: Option<Vec<String>>,
    pub(crate) root: PathBuf,
    pub(crate) vars: BTreeMap<String, String>,
    pub(crate) memory: i64,
    pub(crate) layers: Vec<PathBuf>,
    pub(crate) log_group: String,
    pub(crate) log_stream: String,
    /// `http://127.0.0.1:4566`: where the function's SDK reaches the other services.
    pub(crate) endpoint: String,
    pub(crate) shims: PathBuf,
    pub(crate) creds: Creds,
    pub(crate) interpreters: Interpreters,
    pub(crate) json_logs: bool,
}

/// The program and arguments to run, or why they cannot be had.
///
/// # Errors
/// An interpreter or `bootstrap` that is not there.
pub(crate) fn launch(spec: &Spec) -> Result<(PathBuf, Vec<String>), String> {
    if let Some(line) = &spec.command {
        let (program, args) = line.split_first().ok_or("Command is empty")?;
        return Ok((PathBuf::from(program), args.to_vec()));
    }
    let rt = spec.runtime.as_str();
    let shim = |file: &str| spec.shims.join(file).to_string_lossy().into_owned();
    let handler = spec.handler.clone();
    let it = &spec.interpreters;
    if rt.starts_with("provided") || rt == "go1.x" {
        return entrypoint(&spec.root).map(|p| (p, Vec::new()));
    }
    if rt.starts_with("python") {
        return Ok((
            tool(it.python.as_deref(), "python3"),
            vec![shim("bootstrap.py"), handler],
        ));
    }
    if rt.starts_with("nodejs") {
        return Ok((
            tool(it.node.as_deref(), "node"),
            vec![shim("bootstrap.mjs"), handler],
        ));
    }
    if rt.starts_with("ruby") {
        return Ok((
            tool(it.ruby.as_deref(), "ruby"),
            vec![shim("bootstrap.rb"), handler],
        ));
    }
    if rt.starts_with("java") {
        let cp = format!(
            "{}:{}",
            spec.root.display(),
            spec.root.join("lib").join("*").display()
        );
        let args = [
            "-cp",
            &cp,
            "com.amazonaws.services.lambda.runtime.api.client.AWSLambda",
            &handler,
        ];
        return Ok((
            tool(it.java.as_deref(), "java"),
            args.map(str::to_owned).to_vec(),
        ));
    }
    if rt.starts_with("dotnet") {
        let assembly = handler.split("::").next().unwrap_or(&handler);
        let dll = spec
            .root
            .join(format!("{assembly}.dll"))
            .to_string_lossy()
            .into_owned();
        return Ok((tool(it.dotnet.as_deref(), "dotnet"), vec![dll]));
    }
    Err(format!(
        "The runtime {rt} cannot be run locally; set Command to the launch line of a process that speaks the Runtime API."
    ))
}

fn tool(configured: Option<&str>, default: &str) -> PathBuf {
    PathBuf::from(configured.unwrap_or(default))
}

/// The `bootstrap` of a `provided` runtime: a file of that name in the code directory, or the code itself when it is one file.
fn entrypoint(root: &Path) -> Result<PathBuf, String> {
    if root.is_file() {
        return Ok(root.to_path_buf());
    }
    let boot = root.join("bootstrap");
    if boot.is_file() {
        Ok(boot)
    } else {
        Err("Couldn't find valid bootstrap(s): [/var/task/bootstrap /opt/bootstrap]".to_owned())
    }
}

/// The directories of a layer that a runtime reads, by variable: `python/` for Python, `nodejs/node_modules` for Node...
fn layer_paths(layer: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut out = vec![
        ("PYTHONPATH", layer.join("python")),
        ("NODE_PATH", layer.join("nodejs").join("node_modules")),
        ("RUBYLIB", layer.join("ruby").join("lib")),
        ("PATH", layer.join("bin")),
        ("LD_LIBRARY_PATH", layer.join("lib")),
        ("DYLD_LIBRARY_PATH", layer.join("lib")),
    ];
    if let Ok(read) = std::fs::read_dir(layer.join("python").join("lib")) {
        for entry in read.filter_map(Result::ok) {
            out.push(("PYTHONPATH", entry.path().join("site-packages")));
        }
    }
    if let Ok(read) = std::fs::read_dir(layer.join("ruby").join("gems")) {
        for entry in read.filter_map(Result::ok) {
            out.push(("GEM_PATH", entry.path()));
        }
    }
    out
}

fn prepend(env: &mut BTreeMap<String, String>, key: &str, dirs: &[PathBuf]) {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut parts: Vec<String> = dirs
        .iter()
        .map(|d| d.to_string_lossy().into_owned())
        .collect();
    if let Some(old) = env.get(key).filter(|v| !v.is_empty()) {
        parts.push(old.clone());
    }
    env.insert(key.to_owned(), parts.join(sep));
}

/// The environment of the process, built in passes that overwrite each other: the host's (without its `AWS_` variables), Lambda's
/// own, the endpoints that lead a function's SDK back here, and the function's own variables; layer search paths come last and
/// are only ever put in front of what is there.
pub(crate) fn environment(spec: &Spec, api: &str) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars()
        .filter(|(k, _)| !k.starts_with("AWS_"))
        .collect();
    let mut set = |k: &str, v: String| {
        env.insert(k.to_owned(), v);
    };
    set("AWS_LAMBDA_RUNTIME_API", api.to_owned());
    set("_HANDLER", spec.handler.clone());
    set("AWS_LAMBDA_FUNCTION_NAME", spec.name.clone());
    set("AWS_LAMBDA_FUNCTION_VERSION", spec.version.clone());
    set("AWS_LAMBDA_FUNCTION_MEMORY_SIZE", spec.memory.to_string());
    set("AWS_LAMBDA_LOG_GROUP_NAME", spec.log_group.clone());
    set("AWS_LAMBDA_LOG_STREAM_NAME", spec.log_stream.clone());
    set("AWS_LAMBDA_INITIALIZATION_TYPE", "on-demand".to_owned());
    set("AWS_EXECUTION_ENV", format!("AWS_Lambda_{}", spec.runtime));
    set("AWS_REGION", spec.region.clone());
    set("AWS_DEFAULT_REGION", spec.region.clone());
    set("LAMBDA_TASK_ROOT", spec.root.to_string_lossy().into_owned());
    set(
        "LAMBDA_RUNTIME_DIR",
        spec.shims.to_string_lossy().into_owned(),
    );
    set("TZ", ":UTC".to_owned());
    set("AWS_ACCESS_KEY_ID", spec.creds.key.clone());
    set("AWS_SECRET_ACCESS_KEY", spec.creds.secret.clone());
    set("AWS_SESSION_TOKEN", spec.creds.token.clone());
    set("AWS_ENDPOINT_URL", spec.endpoint.clone());
    if spec.json_logs {
        set("AWS_LAMBDA_LOG_FORMAT", "JSON".to_owned());
    }
    for (k, v) in &spec.vars {
        env.insert(k.clone(), v.clone());
    }
    let mut by_var: BTreeMap<&str, Vec<PathBuf>> = BTreeMap::new();
    for layer in spec.layers.iter().rev() {
        for (var, dir) in layer_paths(layer) {
            if dir.exists() {
                by_var.entry(var).or_default().push(dir);
            }
        }
    }
    for (var, dirs) in by_var {
        prepend(&mut env, var, &dirs);
    }
    let layer_list: Vec<String> = spec
        .layers
        .iter()
        .map(|l| l.to_string_lossy().into_owned())
        .collect();
    env.insert("LAMBDA_LAYERS_DIRS".to_owned(), layer_list.join(":"));
    env
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn spec(runtime: &str) -> Spec {
        Spec {
            name: "f".into(),
            version: "$LATEST".into(),
            region: "us-east-1".into(),
            runtime: runtime.into(),
            handler: "index.handler".into(),
            command: None,
            root: PathBuf::from("/code"),
            vars: BTreeMap::new(),
            memory: 128,
            layers: Vec::new(),
            log_group: "/aws/lambda/f".into(),
            log_stream: "s".into(),
            endpoint: "http://127.0.0.1:4566".into(),
            shims: PathBuf::from("/shims"),
            creds: Creds {
                key: "ASIAX".into(),
                secret: "s".into(),
                token: "t".into(),
                expires_ms: 0,
            },
            interpreters: Interpreters::default(),
            json_logs: false,
        }
    }

    #[test]
    fn each_runtime_family_has_its_launch_line() {
        let (program, args) = launch(&spec("python3.12")).unwrap();
        assert_eq!(
            (program, args),
            (
                PathBuf::from("python3"),
                vec!["/shims/bootstrap.py".to_owned(), "index.handler".to_owned()]
            )
        );
        assert_eq!(
            launch(&spec("nodejs22.x")).unwrap().0,
            PathBuf::from("node")
        );
        assert!(
            launch(&spec("provided.al2023"))
                .unwrap_err()
                .contains("bootstrap")
        );
        assert!(launch(&spec("cobol")).is_err());
        let mut own = spec("provided.al2023");
        own.command = Some(vec!["deno".into(), "run".into()]);
        assert_eq!(launch(&own).unwrap().1, vec!["run".to_owned()]);
    }

    #[test]
    fn the_function_wins_over_lambdas_own_variables_and_host_aws_variables_are_dropped() {
        let mut s = spec("python3.12");
        s.vars
            .insert("AWS_ENDPOINT_URL".into(), "http://elsewhere".into());
        s.vars.insert("GREETING".into(), "hi".into());
        let env = environment(&s, "127.0.0.1:9");
        assert_eq!(
            env.get("AWS_ENDPOINT_URL").map(String::as_str),
            Some("http://elsewhere")
        );
        assert_eq!(env.get("GREETING").map(String::as_str), Some("hi"));
        assert_eq!(
            env.get("AWS_LAMBDA_RUNTIME_API").map(String::as_str),
            Some("127.0.0.1:9")
        );
        assert_eq!(env.get("TZ").map(String::as_str), Some(":UTC"));
    }
}
