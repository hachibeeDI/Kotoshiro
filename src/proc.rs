//! 子プロセス起動の集約点。git / claude はここを通す。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone)]
pub struct Cmd {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
    stdin: Option<String>,
    timeout: Option<Duration>,
}

#[derive(Debug)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            stdin: None,
            timeout: None,
        }
    }

    pub fn args<I, S>(mut self, it: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(it.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, p: impl AsRef<Path>) -> Self {
        self.cwd = Some(p.as_ref().to_path_buf());
        self
    }

    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.env.push((k.into(), v.into()));
        self
    }

    pub fn stdin(mut self, s: impl Into<String>) -> Self {
        self.stdin = Some(s.into());
        self
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn run(&self) -> Result<Output> {
        let mut c = Command::new(&self.program);
        c.args(&self.args)
            .stdin(if self.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &self.cwd {
            c.current_dir(cwd);
        }
        for (k, v) in &self.env {
            c.env(k, v);
        }
        let mut child = c
            .spawn()
            .with_context(|| format!("spawn `{}`", self.display()))?;

        if let Some(input) = self.stdin.clone() {
            let mut si = child.stdin.take().expect("piped stdin");
            std::thread::spawn(move || {
                let _ = si.write_all(input.as_bytes());
            });
        }
        let mut so = child.stdout.take().expect("piped stdout");
        let mut se = child.stderr.take().expect("piped stderr");
        let out_t = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = so.read_to_end(&mut b);
            b
        });
        let err_t = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = se.read_to_end(&mut b);
            b
        });

        let status = match self.timeout {
            None => child.wait()?,
            Some(t) => {
                let deadline = Instant::now() + t;
                loop {
                    if let Some(st) = child.try_wait()? {
                        break st;
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        bail!("`{}` timed out after {:?}", self.display(), t);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        };
        let stdout = String::from_utf8_lossy(&out_t.join().unwrap_or_default()).into_owned();
        let stderr = String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned();
        Ok(Output {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    }

    /// 終了コード 0 以外をエラーにする。
    pub fn ok(&self) -> Result<Output> {
        let out = self.run()?;
        if out.code != 0 {
            bail!(
                "`{}` failed ({}): {}",
                self.display(),
                out.code,
                out.stderr.trim()
            );
        }
        Ok(out)
    }
}
