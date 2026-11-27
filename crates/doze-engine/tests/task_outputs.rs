//! A task's `outputs`: what a build makes. Up to date by its inputs, a task still runs when one is gone, so an app that
//! runs a built program never starts without it.

mod common;

use common::*;
use doze_engine::tasks::TaskSpec;
use doze_engine::{Cause, Engine, Service};

fn task(d: &Dir, args: &[&str]) -> TaskSpec {
    TaskSpec::new(run(args), d.path())
}

/// A task whose inputs did not change still runs when one of its `outputs` is gone (a build whose program was
/// deleted), and says so; while the output is there it is up to date. A manual task is not run for a missing output.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_output_runs_the_task_again() {
    let d = Dir::new("taskoutputs");
    let (built, src) = (d.file("app.bin"), d.path().join("src"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.go"), "package main").unwrap();
    let mut manual = task(&d, &["append", &d.file("m.bin").to_string_lossy()]).outputs(&["m.bin"]);
    manual.manual = true;
    let e = Engine::builder("t", d.path())
        .service(Service::task(
            "build",
            task(&d, &["append", &built.to_string_lossy()])
                .inputs(&["src/*.go"])
                .outputs(&["app.bin"]),
        ))
        .service(Service::task("by-hand", manual))
        .build()
        .unwrap();
    e.wake("build", Cause::Command).await.unwrap();
    e.wake("build", Cause::Command).await.unwrap();
    assert_eq!(lines(&built), 1, "up to date while its output is there");
    std::fs::remove_file(&built).unwrap();
    e.wake("build", Cause::Command).await.unwrap();
    assert_eq!(lines(&built), 1, "made again");
    assert_eq!(e.task_record("build").unwrap().why, "app.bin is missing");
    e.wake("by-hand", Cause::Command).await.unwrap();
    assert!(!d.file("m.bin").exists(), "manual: a wake does not run it");
    e.shutdown().await;
}
