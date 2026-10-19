use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn gray_home() -> PathBuf {
    std::env::var_os("GRAY_HOME")
        .filter(|v| !v.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".gray"))
}

pub fn qol_dir() -> PathBuf {
    gray_home().join("qol")
}

pub fn tilde(p: &Path) -> String {
    let home = home();
    match p.strip_prefix(&home) {
        Ok(rest) if home != Path::new("/") => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

#[cfg(unix)]
pub fn which(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

// Executability on Windows is the extension: `name` as given, else
// `name` + each PATHEXT suffix (cmd's own rule, defaulting to the usual set).
#[cfg(windows)]
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = std::env::var("PATHEXT")
        .map(|v| {
            v.split(';')
                .map(|e| e.trim().to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect()
        })
        .unwrap_or_else(|_| {
            [".com", ".exe", ".bat", ".cmd"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        });
    let mut names = vec![name.to_string()];
    if Path::new(name).extension().is_none() {
        names.extend(exts.iter().map(|e| format!("{name}{e}")));
    }
    std::env::split_paths(&path)
        .flat_map(|d| names.iter().map(|n| d.join(n)).collect::<Vec<_>>())
        .find(|p| p.metadata().is_ok_and(|m| m.is_file()))
}

#[cfg(not(any(unix, windows)))]
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.metadata().is_ok_and(|m| m.is_file()))
}

pub fn gray_bin() -> String {
    std::env::var("GRAY_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "gray".to_string())
}

pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("qol-tmp-{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
