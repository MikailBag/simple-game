use anyhow::{bail, Context, Result};
use std::{path::Path, process::Command};

#[derive(Debug)]
enum CodeKind {
    Python,
}

fn detect_kind(path: &Path) -> Result<CodeKind> {
    if path.extension() == Some(std::ffi::OsStr::new("py")) {
        return Ok(CodeKind::Python);
    }

    bail!("could not detect code kind for {}", path.display())
}

fn main() -> anyhow::Result<()> {
    let path = match std::env::args_os().nth(1) {
        None => {
            eprintln!("path to file executed not given");
            std::process::exit(1);
        }
        Some(x) => std::path::PathBuf::from(x),
    };
    let kind = detect_kind(&path)?;
    eprintln!("{} detected as {:?}", path.display(), kind);
    exec(&path, kind)?;
    Ok(())
}

fn exec(path: &Path, kind: CodeKind) -> anyhow::Result<()> {
    match kind {
        CodeKind::Python => {
            let mut cmd = Command::new("python3");
            cmd.arg(path);
            let st = cmd
                .status()
                .context("failed to launch python interpreter")?;
            if !st.success() {
                bail!("script failed: return {}", st);
            }
        }
    }
    Ok(())
}
