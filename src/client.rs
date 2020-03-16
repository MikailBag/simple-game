use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Eq, PartialEq, Debug)]
enum State {
    /// Initializing
    Init,
    /// Error
    Error,
    /// Client waits for game start
    Wait,
    /// Client selects step
    Step,
    /// Client waits for round results
    PostStep,
    /// Client finished
    End
}
#[derive(Debug)]
pub(crate) struct Client {
    child: std::process::Child,
    stdout: Arc<Mutex<std::io::BufReader<std::process::ChildStdout>>>,
    stdin: Arc<Mutex<std::io::BufWriter<std::process::ChildStdin>>>,
    name: String,
    state: State,
    num: u32,
}

struct ReadLineState {
    buf: String,
    done: bool,
    error: bool,
}

impl std::fmt::Display for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Client(path = {})", &self.name)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Client {
    fn from_child(mut child: std::process::Child, path: &Path) -> Client {
        let stdout = Arc::new(Mutex::new(std::io::BufReader::new(
            child.stdout.take().unwrap(),
        )));
        let stdin = Arc::new(Mutex::new(std::io::BufWriter::new(
            child.stdin.take().unwrap(),
        )));
        Client {
            child,
            name: path.display().to_string(),
            state: State::Init,
            stdout,
            num: 0xDEADBEEF,
            stdin,
        }
    }

    fn new_on_host(path: &str) -> Result<Client> {
        let child = std::process::Command::new(std::env::current_exe()?)
            .arg(path)
            .env("__RUN__", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        Ok(Self::from_child(child, std::path::Path::new(path)))
    }

    fn new_docker(path: &str, image: &str) -> Result<Client> {
        let mut inner_path = std::path::PathBuf::new();
        inner_path.push("/src");
        let path = std::path::Path::new(path);
        let file_name = match path.file_name() {
            Some(name) => name,
            None => bail!("path does not contain filename"),
        };
        inner_path.push(file_name);
        let mount_flag = format!(
            "--mount=type=bind,source={},target={},readonly=true",
            path.canonicalize()
                .context("failed to resolve full path")?
                .display(),
            inner_path.display()
        );
        let child = std::process::Command::new("docker")
            .arg("run")
            .arg("--interactive")
            .arg("--rm")
            .arg("--env=__RUN__=1")
            .arg(mount_flag)
            .arg(image)
            .arg(inner_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        Ok(Self::from_child(child, path))
    }

    pub(crate) fn new(path: &str, image: Option<&str>) -> Result<Client> {
        match image {
            Some(img) => Self::new_docker(path, img),
            None => Self::new_on_host(path),
        }
    }

    pub(crate) fn is_init(&self) -> bool {
        self.state == State::Init
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    fn read_line(&mut self) -> Result<String> {
        let state = ReadLineState {
            buf: String::new(),
            done: false,
            error: false,
        };
        let state = Arc::new(Mutex::new(state));
        let stdout = Arc::clone(&self.stdout);
        let ch_state = Arc::clone(&state);
        let name = self.name.clone();
        let handle = std::thread::spawn(move || {
            let state = ch_state;
            let mut buf = String::new();
            let err = stdout.lock().unwrap().read_line(&mut buf).err();
            let mut st = state.lock().unwrap();
            st.buf = buf.trim().to_string();
            st.done = true;
            if let Some(err) = err {
                eprintln!("client {}: i/o error: {}", name, err);
                st.error = true;
            }
        });
        let timeout_ms = match self.state {
            State::Init => 10000,
            _ => 1000,
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let mut st = state.lock().unwrap();
            if st.done {
                handle.join().unwrap();
                if st.error {
                    self.err();
                    bail!("reader thread errored");
                }
                break Ok(std::mem::take(&mut st.buf));
            }
            if std::time::Instant::now() > deadline {
                self.err();
                bail!("deadline violated");
            }
        }
    }

    pub(crate) fn err(&mut self) {
        self.state = State::Error;
        self.num = u32::max_value();
    }

    fn is_err(&self) -> bool {
        self.state == State::Error
    }

    pub(crate) fn poll(&mut self) {
        match self.state {
            State::Error | State::Wait | State::PostStep | State::End => return,
            State::Init | State::Step => (),
        };
        let line = match self.read_line() {
            Ok(l) => l,
            Err(err) => {
                println!("client {}: failed to read line: {}", &self.name, err);
                self.err();
                return;
            }
        };
        match self.state {
            State::Init => {
                if line == "ready" {
                    self.state = State::Wait;
                } else {
                    println!(
                        "client {}: unknown message when waiting for `ready`: {}",
                        &self.name, line
                    );
                    self.err();
                }
            }
            State::Step => {
                let guess: u32 = match line.parse() {
                    Ok(g) => g,
                    Err(err) => {
                        println!(
                            "client {}: got '{}' which is not a number: {}",
                            &self.name, line, err
                        );
                        self.err();
                        return;
                    }
                };
                self.num = guess;
                self.state = State::PostStep;
            }
            _ => unreachable!(),
        }
    }

    pub(crate) fn get_num(&mut self) -> u32 {
        self.num
    }

    pub(crate) fn send_end(&mut self) {
        if self.is_err() {
            return;
        }
        self.send_line(b"end\n".to_vec());
        self.state = State::End;
    }

    pub(crate) fn send_game(&mut self) {
        if self.is_err() {
            return;
        }
        self.send_line(b"game\n".to_vec());
        self.state = State::Step;
    }

    pub(crate) fn send_nums(&mut self, num: &[u32]) {
        if self.is_err() {
            return;
        }
        let mut buf = Vec::new();
        for x in num {
            if !buf.is_empty() {
                buf.push(b' ');
            }
            write!(buf, "{}", x).unwrap();
        }
        write!(buf, "\n").unwrap();
        self.state = State::Wait;
        self.send_line(buf);
    }

    fn send_line(&mut self, line: Vec<u8>) {
        let done = Arc::new(AtomicBool::new(false));
        let err = Arc::new(AtomicBool::new(false));
        let name = self.name.clone();
        let stdin = Arc::clone(&self.stdin);
        {
            let done = Arc::clone(&done);
            let err = Arc::clone(&err);
            std::thread::spawn(move || {
                let mut stdin = stdin.lock().unwrap();
                let mut is_err = false;
                if let Err(err) = stdin.write_all(&line) {
                    eprintln!("client {}: failed to write line: {}", name, err);
                    is_err = true;
                } else if let Err(err) = stdin.flush() {
                    eprintln!("client {}: failed to flush: {}", name, err);
                    is_err = true;
                }
                if is_err {
                    err.store(true, Ordering::SeqCst);
                }
                done.store(true, Ordering::SeqCst);
            });
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
        loop {
            if std::time::Instant::now() > deadline {
                eprintln!("client {}: send_line: timeout", &self.name);
                self.err();
                return;
            }
            if !done.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(30));
                continue;
            }
            if err.load(Ordering::SeqCst) {
                self.err();
            }
            return;
        }
    }
}
