//! Whether a function can be launched on this host, asked when it is created or its runtime or code changes. The answer is a warning
//! in the log and nothing more: AWS accepts the function (whether the host has a `python3` is not a fact AWS knows), so the call
//! succeeds, and the first invocation fails with `Runtime.LaunchError` naming the same reason. This is the Go edition's `CheckRuntime`:
//! the interpreter of the runtime family (the configured one, or the one on `PATH`), the AWS Java runtime interface client in a Java
//! package, and the self-hosting assembly of a .NET one (a class library cannot speak the Runtime API).

use std::path::{Path, PathBuf};

use aws_model_lambda::Operation;
use aws_wire::service::Caller;
use serde_json::Value;

use super::spec::Interpreters;
use crate::Inner;
use crate::core::{Answer, Ask};
use crate::resolve::Resolving;

/// The function a call that may make it unlaunchable names: a create, or a change of its configuration or code.
pub(crate) fn checked(op: Operation, caller: &Caller, function: Option<&str>) -> Option<Resolving> {
    let changes = matches!(
        op,
        Operation::CreateFunction
            | Operation::UpdateFunctionConfiguration
            | Operation::UpdateFunctionCode
    );
    Some(Resolving {
        caller: caller.clone(),
        function: function.filter(|_| changes)?.to_owned(),
        qualifier: None,
        source_arn: None,
        draw: 0.0,
        url: false,
        internal: true,
    })
}

impl Inner {
    /// Says in the log why the function, as it now is, could not be launched here, if it could not.
    pub(crate) async fn warn_if_unlaunchable(&self, function: Resolving) {
        let region = function.caller.region.clone();
        let Ok(Answer::Resolved(Ok(rs))) =
            self.ask(&region, Ask::Resolve(Box::new(function))).await
        else {
            return;
        };
        if rs.code.image.is_some() {
            return;
        }
        let runtime = rs
            .config
            .get("Runtime")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let command = rs.config.get("Command").is_some_and(|c| !c.is_null());
        if let Some(why) = check(runtime, &rs.root, command, self.rt.interpreters()) {
            tracing::warn!(function = %rs.name, "{why}; the function is kept, and its first invocation will fail with Runtime.LaunchError");
        }
    }
}

/// The entry class of `aws-lambda-java-runtime-interface-client`.
const JAVA_RIC: &str = "com/amazonaws/services/lambda/runtime/api/client/AWSLambda.class";

/// Why a function of `runtime` with its code at `root` cannot be launched here, if it cannot. `command` is the doze `Command`
/// extension: the caller's own launch line, which is not second-guessed.
pub(crate) fn check(
    runtime: &str,
    root: &Path,
    command: bool,
    it: &Interpreters,
) -> Option<String> {
    if command || runtime.starts_with("provided") || runtime == "go1.x" || runtime.is_empty() {
        return None;
    }
    let (family, configured, program) = if runtime.starts_with("python") {
        ("python", it.python.as_deref(), "python3")
    } else if runtime.starts_with("nodejs") {
        ("nodejs", it.node.as_deref(), "node")
    } else if runtime.starts_with("ruby") {
        ("ruby", it.ruby.as_deref(), "ruby")
    } else if runtime.starts_with("java") {
        ("java", it.java.as_deref(), "java")
    } else if runtime.starts_with("dotnet") {
        ("dotnet", it.dotnet.as_deref(), "dotnet")
    } else {
        return Some(format!(
            "the runtime {runtime} cannot be run locally; set Command to the launch line of a process that speaks the Runtime API"
        ));
    };
    if let Some(why) = interpreter(family, configured, program) {
        return Some(why);
    }
    match family {
        "java" => java_client(root),
        "dotnet" => dotnet_entry(root),
        _ => None,
    }
}

fn interpreter(family: &str, configured: Option<&str>, program: &str) -> Option<String> {
    match configured {
        Some(path) if Path::new(path).is_file() => None,
        Some(path) => Some(format!(
            "[lambda.runtimes] {family} = {path:?} does not exist"
        )),
        None if on_path(program).is_some() => None,
        None => Some(format!(
            "runtime family {family} needs {program} on PATH, or [lambda.runtimes] {family} = \"/path/to/{program}\" in the config"
        )),
    }
}

/// `program` as the shell would find it.
fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

fn java_client(root: &Path) -> Option<String> {
    if root.join(JAVA_RIC).is_file() {
        return None;
    }
    let jars = |dir: PathBuf| {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jar"))
            .collect::<Vec<_>>()
    };
    let found = jars(root.to_path_buf())
        .into_iter()
        .chain(jars(root.join("lib")))
        .any(|jar| {
            std::fs::read(&jar)
                .map(|bytes| crate::zipread::names(&bytes).iter().any(|n| n == JAVA_RIC))
                .unwrap_or(false)
        });
    (!found).then(|| "a Java function needs the AWS runtime interface client in its package: add com.amazonaws:aws-lambda-java-runtime-interface-client (Maven) or implementation(\"com.amazonaws:aws-lambda-java-runtime-interface-client:2.+\") (Gradle) and ship it shaded or under lib/. It binds a Linux-only native client, so on macOS use an explicit Command".to_owned())
}

/// A published .NET executable leaves `<name>.runtimeconfig.json` beside `<name>.dll`, with the runtime support assembly.
fn dotnet_entry(root: &Path) -> Option<String> {
    let support = root.join("Amazon.Lambda.RuntimeSupport.dll").is_file();
    let executable = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".runtimeconfig.json").map(str::to_owned)
        })
        .any(|stem| root.join(format!("{stem}.dll")).is_file());
    (!(support && executable)).then(|| "a .NET function needs a self-hosting assembly: reference the NuGet package Amazon.Lambda.RuntimeSupport, call LambdaBootstrap from Main, and publish it as an executable in the code directory".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("doze-lambda-check-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&d));
        std::fs::create_dir_all(&d).unwrap_or_default();
        d
    }

    #[test]
    fn a_missing_interpreter_and_an_incomplete_package_are_named() {
        let none = Interpreters {
            python: Some("/no/such/python3".to_owned()),
            java: Some("/bin/sh".to_owned()),
            dotnet: Some("/bin/sh".to_owned()),
            ..Interpreters::default()
        };
        let root = dir("pkg");
        let why = check("python3.12", &root, false, &none).unwrap_or_default();
        assert!(why.contains("/no/such/python3"), "{why}");
        assert_eq!(
            check("python3.12", &root, true, &none),
            None,
            "Command is the caller's"
        );
        assert_eq!(check("provided.al2023", &root, false, &none), None);
        let java = check("java21", &root, false, &none).unwrap_or_default();
        assert!(
            java.contains("aws-lambda-java-runtime-interface-client"),
            "{java}"
        );
        let jar = crate::zipread::build(&[(JAVA_RIC, b"class", 0o644)]);
        std::fs::create_dir_all(root.join("lib")).unwrap_or_default();
        std::fs::write(root.join("lib").join("ric.jar"), jar).unwrap_or_default();
        assert_eq!(check("java21", &root, false, &none), None);
        let dotnet = check("dotnet8", &root, false, &none).unwrap_or_default();
        assert!(dotnet.contains("Amazon.Lambda.RuntimeSupport"), "{dotnet}");
        for f in [
            "App.dll",
            "App.runtimeconfig.json",
            "Amazon.Lambda.RuntimeSupport.dll",
        ] {
            std::fs::write(root.join(f), b"x").unwrap_or_default();
        }
        assert_eq!(check("dotnet8", &root, false, &none), None);
        drop(std::fs::remove_dir_all(&root));
    }
}
