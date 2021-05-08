//! Utility functions for finding the `cargo bench` command that was used to
//! build the currently running executable and running the same command with
//! additional parameters.
use serde::{de, Deserialize};
use std::{
    env,
    ffi::OsString,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::utils::Serde;

#[derive(Debug)]
pub struct CompiledExecutable {
    pub path: PathBuf,
    pub library_paths: Vec<PathBuf>,
}

pub fn compile_self(modify_cmd: impl FnOnce(&mut Command) -> &mut Command) -> CompiledExecutable {
    let (cargo_path, package_path, cargo_args) = super::cargo::cargo_bench_path_args()
        .expect("could not determine the cargo command used to build this target");

    std::env::set_current_dir(package_path).expect("could not cd to the package directory");

    let mut cargo = modify_cmd(&mut Command::new(cargo_path).args(cargo_args).args(&[
        "--no-run",
        "--message-format",
        "json-render-diagnostics",
    ]))
    .stdin(Stdio::null())
    .stderr(Stdio::inherit()) // Cargo writes its normal compile output to stderr
    .stdout(Stdio::piped()) // Capture the JSON messages on stdout
    .spawn()
    .expect("could not launch cargo");

    let cargo_stdout = BufReader::new(cargo.stdout.take().unwrap());

    let mut path = None;
    let mut library_paths = Vec::new();

    for line in cargo_stdout.lines() {
        let msg: Message = serde_json_core::from_str(&line.unwrap()).unwrap().0;
        match msg {
            Message::CompilerArtifact { target, executable } => {
                if target.kind.0.iter().any(|kind| kind.0 == "bench") {
                    if let Some(executable) = executable {
                        path = Some(json_unescape(&executable.0).into());
                    }
                }
            }
            Message::BuildScriptExecuted { linked_paths } => {
                for path in linked_paths.0 {
                    let path = json_unescape(&path.0)
                        .replace("dependency=", "")
                        .replace("crate=", "")
                        .replace("native=", "")
                        .replace("framework=", "")
                        .replace("all=", "");
                    let path = PathBuf::from(path);
                    library_paths.push(path);
                }
            }
            _ => (),
        }
    }

    cargo.wait().expect("cargo failed");

    CompiledExecutable {
        path: path.expect("cargo did not return artifact path"),
        library_paths,
    }
}

fn cargo_bench_path_args() -> Result<(PathBuf, PathBuf, Vec<OsString>), &'static str> {
    let cargo = env::var_os("CARGO").ok_or("$CARGO is not set")?;

    let package_path = env::var_os("CARGO_MANIFEST_DIR").ok_or("$CARGO_MANIFEST_DIR is not set")?;

    let mut exe_path =
        env::current_exe().map_err(|_| "could not find the current executable name")?;
    exe_path.set_extension("");
    let exe_name = exe_path
        .file_name()
        .unwrap()
        .to_str()
        .ok_or("the current executable name is not a valid UTF-8 string")?;

    // Remove the crate disambiguator, (probably) leaving only the crate name
    // This is unstable but the best thing we can do for now
    let i = exe_name
        .rfind("-")
        .ok_or("could not locate the crate disambiguator in the current executable name")?;
    let target_name = &exe_name[0..i];

    Ok((
        cargo.into(),
        package_path.into(),
        vec!["bench".into(), "--bench".into(), target_name.into()],
    ))
}

// These structs match the parts of Cargo's message format that we care about.
#[derive(Deserialize, Debug)]
struct Target {
    name: Serde<String>,
    kind: Serde<Vec<Serde<String>>>,
}

/// Enum listing out the different types of messages that Cargo can send. We only care about the
/// compiler-artifact message.
#[derive(Debug)]
enum Message {
    CompilerArtifact {
        target: Target,
        // `PathBuf` does not have `impl Deserialize` when `serde` is built
        // without `serde/std`
        executable: Option<Serde<String>>,
    },

    CompilerMessage {},

    BuildScriptExecuted {
        linked_paths: Serde<Vec<Serde<String>>>,
    },

    BuildFinished {},
}

#[derive(Deserialize)]
struct MessageFlat {
    reason: MessageReason,
    target: Option<Target>,
    executable: Option<Serde<String>>,
    linked_paths: Option<Serde<Vec<Serde<String>>>>,
}

#[derive(Deserialize)]
enum MessageReason {
    #[serde(rename = "compiler-artifact")]
    CompilerArtifact,
    #[serde(rename = "compiler-message")]
    CompilerMessage,
    #[serde(rename = "build-script-executed")]
    BuildScriptExecuted,
    #[serde(rename = "build-finished")]
    BuildFinished,
}

// Deserializing tagged enums isn't supported by `serde` when compiled
// without `alloc`
impl<'de> de::Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let flat = MessageFlat::deserialize(deserializer)?;

        match flat.reason {
            MessageReason::CompilerArtifact => Ok(Self::CompilerArtifact {
                target: flat.target.ok_or(de::Error::missing_field("target"))?,
                executable: flat.executable,
            }),
            MessageReason::CompilerMessage => Ok(Self::CompilerMessage {}),
            MessageReason::BuildScriptExecuted => Ok(Self::BuildScriptExecuted {
                linked_paths: flat
                    .linked_paths
                    .ok_or(de::Error::missing_field("linked_paths"))?,
            }),
            MessageReason::BuildFinished => Ok(Self::BuildFinished {}),
        }
    }
}

/// Unescape a JSON string. (`serde_json_core` doesn't unescape them.)
fn json_unescape(x: &str) -> String {
    let mut out = String::with_capacity(x.len());
    let mut it = x.split("\\");
    out.push_str(it.next().unwrap());
    while let Some(part) = it.next() {
        if part.len() == 0 {
            // It's double backslash
            let rest = it.next().expect("incomplete JSON string escape sequence");
            out.push_str("\\");
            out.push_str(rest);
        } else if let Some(_) = part.strip_prefix("u") {
            todo!()
        } else {
            let (rest, ch) = if let Some(rest) = part.strip_prefix("\"") {
                (rest, "\"")
            } else if let Some(rest) = part.strip_prefix("/") {
                (rest, "/")
            } else if let Some(rest) = part.strip_prefix("b") {
                (rest, "\x08")
            } else if let Some(rest) = part.strip_prefix("f") {
                (rest, "\x0c")
            } else if let Some(rest) = part.strip_prefix("n") {
                (rest, "\n")
            } else if let Some(rest) = part.strip_prefix("r") {
                (rest, "\r")
            } else if let Some(rest) = part.strip_prefix("t") {
                (rest, "\t")
            } else {
                panic!("unrecognized JSON string escape sequence");
            };

            out.push_str(ch);
            out.push_str(rest);
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_unescape() {
        assert_eq!(json_unescape(""), "");
        assert_eq!(json_unescape("a"), "a");
        assert_eq!(json_unescape(r"\n"), "\n");
        assert_eq!(json_unescape(r"a\ra"), "a\ra");
        assert_eq!(json_unescape(r"a\r\na"), "a\r\na");
        assert_eq!(json_unescape(r"a\\\r\\a"), "a\\\r\\a");
    }
}
