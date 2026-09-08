#![cfg(target_os = "linux")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn temporary_root(test_name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "volund-{test_name}-{}-{unique}",
        std::process::id()
    ))
}

fn spawn_job(root: &Path, input: &Path, timeout_seconds: u64) -> Child {
    let converter = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/slow-converter.sh");
    Command::new(env!("CARGO_BIN_EXE_volundd"))
        .arg("convert")
        .arg("--input")
        .arg(input)
        .arg("--timeout-seconds")
        .arg(timeout_seconds.to_string())
        .arg("--derived-root")
        .arg(root.join("derived"))
        .arg("--scratch-root")
        .arg(root.join("scratch"))
        .arg("--converter")
        .arg(converter)
        .spawn()
        .expect("spawn volundd job")
}

fn ready_job_count(root: &Path) -> usize {
    fs::read_dir(root.join("derived/jobs"))
        .expect("read ready jobs")
        .count()
}

fn failed_job_metadata(root: &Path) -> String {
    let failed = fs::read_dir(root.join("derived/failed"))
        .expect("read failed jobs")
        .next()
        .expect("one failed job")
        .expect("failed job entry")
        .path();
    fs::read_to_string(failed.join("job.json")).expect("read failed job metadata")
}

fn wait(child: &mut Child) -> ExitStatus {
    child.wait().expect("wait for volundd job")
}

#[test]
fn two_submissions_are_serialized_by_the_global_lock() {
    let root = temporary_root("serial");
    fs::create_dir_all(&root).expect("create test root");
    let input = root.join("fixture.step");
    fs::write(&input, "synthetic input").expect("write input");

    let started = Instant::now();
    let mut first = spawn_job(&root, &input, 30);
    thread::sleep(Duration::from_millis(100));
    let mut second = spawn_job(&root, &input, 30);
    assert!(wait(&mut first).success());
    assert!(wait(&mut second).success());
    assert!(
        started.elapsed() >= Duration::from_millis(3900),
        "two two-second converters should take about four seconds when serialized"
    );
    assert_eq!(ready_job_count(&root), 2);

    fs::remove_dir_all(root).expect("remove test root");
}

#[test]
fn timeout_moves_the_job_to_failed() {
    let root = temporary_root("timeout");
    fs::create_dir_all(&root).expect("create test root");
    let input = root.join("fixture.step");
    fs::write(&input, "synthetic input").expect("write input");

    let mut job = spawn_job(&root, &input, 1);
    assert!(!wait(&mut job).success());
    let metadata = failed_job_metadata(&root);
    assert!(metadata.contains("\"state\": \"failed\""));
    assert!(metadata.contains("exceeded the 1 second limit"));

    fs::remove_dir_all(root).expect("remove test root");
}
