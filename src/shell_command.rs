//! The `andruide` command in the user's PATH, so terminals (and other agents,
//! with `andruide mcp`) can run it: a link to this executable in
//! /usr/local/bin on macOS (asking for an administrator's password when the
//! folder isn't writable) and ~/.local/bin on Linux; on Windows, this
//! executable's folder added to the user's PATH. When Homebrew or a Linux
//! package installed the app, its link is the one in the PATH; an AppImage
//! links the AppImage file, not its temporary mount.
//!
//! There is no legacy alias beside the link: unlike upstream's muxin (which
//! kept `muxen` from its earlier name), AndruIDE has never shipped under
//! another name, so there is nothing for it to keep working. (Brand rename,
//! Father's directive 11 Nov 2026; the alias collapse broke CI run
//! 43079175313 — alias ended up equal to the link itself.)

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    /// "installed" | "missing" | "other" (something else is there) | "unsupported"
    pub state: &'static str,
    /// Where the command lives: the link, or on Windows the folder in PATH.
    pub path: String,
    /// What runs: this executable, or what the link points to now.
    pub target: Option<String>,
    /// Something to know, in plain words ("Open a new terminal to use it.").
    pub note: Option<String>,
    /// "other" that install may replace: a link to another copy of AndruIDE.
    pub replaceable: bool,
    /// Homebrew or the app's Linux package put it there, and takes it away.
    pub managed: bool,
}

/// Whether `andruide` is in the PATH and runs this copy of AndruIDE.
pub fn status() -> Status {
    let exe = match this_exe() {
        Ok(exe) => exe,
        Err(e) => return Status { state: "unsupported", note: Some(e), ..Default::default() },
    };
    imp::status(&exe)
}

pub fn install() -> Result<Status, String> {
    let exe = this_exe()?;
    if exe.to_string_lossy().contains("/AppTranslocation/") {
        return Err("macOS is running AndruIDE from a temporary copy. Move AndruIDE to Applications, open it from there, and try again.".into());
    }
    imp::install(&exe)?;
    Ok(imp::status(&exe))
}

pub fn remove() -> Result<Status, String> {
    let exe = this_exe()?;
    imp::remove(&exe)?;
    Ok(imp::status(&exe))
}

fn this_exe() -> Result<PathBuf, String> {
    // An AppImage runs from a mount that's gone once it quits: the command
    // runs the AppImage file instead.
    if cfg!(target_os = "linux")
        && let Some(file) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|f| f.is_file())
    {
        return Ok(std::fs::canonicalize(&file).unwrap_or(file));
    }
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't tell where AndruIDE is: {e}"))?;
    Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// A link of ours: it points to a AndruIDE executable (this one or another
/// copy), not to something else that happens to have the name.
#[cfg(unix)]
fn is_ours(target: &Path) -> bool {
    let s = target.to_string_lossy();
    let name = target.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    name == "andruide" || (name.starts_with("andruide") && name.ends_with(".appimage")) || s.contains("AndruIDE.app/")
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::ErrorKind;

    /// Where the link goes on this OS.
    fn link_path() -> Result<PathBuf, String> {
        if cfg!(target_os = "macos") {
            Ok(PathBuf::from("/usr/local/bin/andruide"))
        } else {
            let home = std::env::var_os("HOME").ok_or("HOME isn't set.")?;
            Ok(PathBuf::from(home).join(".local/bin/andruide"))
        }
    }

    /// Who installed this copy and its link, when that wasn't AndruIDE: Homebrew
    /// (`brew install --cask andruide`), or the .deb, .rpm or Arch package (the
    /// app in /usr/lib/andruide, the link in /usr/bin).
    #[derive(Clone, Copy, PartialEq, Debug)]
    pub(super) enum Manager {
        Homebrew,
        Package,
    }

    fn managed_link(exe: &Path) -> Option<(PathBuf, Manager)> {
        let runs_this = |link: &Path| std::fs::canonicalize(link).is_ok_and(|r| r == exe);
        if cfg!(target_os = "linux") && exe.starts_with("/usr/lib/andruide") && runs_this(Path::new("/usr/bin/andruide")) {
            return Some((PathBuf::from("/usr/bin/andruide"), Manager::Package));
        }
        let prefixes = std::env::var_os("HOMEBREW_PREFIX").map(PathBuf::from).into_iter().chain(["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"].map(PathBuf::from));
        prefixes.filter(|p| p.join("Caskroom/andruide").is_dir()).map(|p| p.join("bin/andruide")).find(|link| runs_this(link)).map(|link| (link, Manager::Homebrew))
    }

    pub fn status(exe: &Path) -> Status {
        if let Some((link, by)) = managed_link(exe) {
            let note = Some(match by {
                Manager::Homebrew => "Homebrew put it there with the app.",
                Manager::Package => "AndruIDE's package put it there with the app.",
            });
            return Status { state: "installed", path: link.display().to_string(), target: Some(exe.display().to_string()), note: note.map(Into::into), replaceable: false, managed: true };
        }
        match link_path() {
            Ok(link) => status_at(&link, exe),
            Err(e) => Status { state: "unsupported", note: Some(e), ..Default::default() },
        }
    }

    pub(super) fn status_at(link: &Path, exe: &Path) -> Status {
        let link = link.to_path_buf();
        let path = link.display().to_string();
        let Ok(meta) = std::fs::symlink_metadata(&link) else {
            return Status { state: "missing", path, ..Default::default() };
        };
        if !meta.file_type().is_symlink() {
            return Status { state: "other", path, note: Some("A different program called andruide is there; AndruIDE leaves it alone.".into()), ..Default::default() };
        }
        let target = std::fs::read_link(&link).ok();
        let resolved = std::fs::canonicalize(&link).ok();
        let shown = target.as_ref().map(|t| t.display().to_string());
        let ours = target.as_deref().is_some_and(is_ours);
        let other = |note: &str, replaceable| Status { state: "other", path: path.clone(), target: shown.clone(), note: Some(note.into()), replaceable, managed: false };
        match resolved {
            Some(r) if r == exe => Status { state: "installed", path: path.clone(), target: shown.clone(), note: in_path_note(&link), ..Default::default() },
            _ if !ours => other("It links to a different program called andruide; AndruIDE leaves it alone.", false),
            None => other("It points to a copy of AndruIDE that isn't there anymore.", true),
            Some(_) => other("It runs another copy of AndruIDE.", true),
        }
    }

    /// Linux: ~/.local/bin is only in PATH when the login that started this
    /// app found it (Ubuntu adds it at login, once it exists).
    fn in_path_note(link: &Path) -> Option<String> {
        if cfg!(target_os = "macos") {
            return None;
        }
        let dir = link.parent()?;
        let in_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir));
        (!in_path).then(|| format!("{} isn't in your PATH yet. It usually is after you log in again; or add it in your shell's profile.", dir.display()))
    }

    pub fn install(exe: &Path) -> Result<(), String> {
        if managed_link(exe).is_some() {
            return Ok(());
        }
        install_at(&link_path()?, exe)
    }

    /// The link in the PATH, pointing at this executable.
    pub(super) fn install_at(link: &Path, exe: &Path) -> Result<(), String> {
        refuse_others(link)?;
        match link_here(link, exe) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::PermissionDenied && cfg!(target_os = "macos") => {
                let dir = link.parent().map(|d| d.display().to_string()).unwrap_or_default();
                as_admin(&format!("mkdir -p {} && ln -sfn {} {}", sh(&dir), sh(&exe.display().to_string()), sh(&link.display().to_string())))
            }
            Err(e) => Err(format!("Couldn't create {}: {e}", link.display())),
        }
    }

    pub fn remove(exe: &Path) -> Result<(), String> {
        match managed_link(exe) {
            Some((_, Manager::Homebrew)) => return Err("Homebrew put it there. It goes when you uninstall AndruIDE with brew uninstall --cask andruide.".into()),
            Some((_, Manager::Package)) => return Err("AndruIDE's package put it there. It goes when you uninstall the package.".into()),
            None => {}
        }
        remove_at(&link_path()?)
    }

    /// The link, when it's ours.
    pub(super) fn remove_at(link: &Path) -> Result<(), String> {
        let ours = |l: &Path| std::fs::symlink_metadata(l).is_ok_and(|m| m.file_type().is_symlink()) && std::fs::read_link(l).is_ok_and(|t| is_ours(&t));
        if std::fs::symlink_metadata(link).is_ok() && !ours(link) {
            return Err(format!("{} isn't a link to AndruIDE, so AndruIDE leaves it alone.", link.display()));
        }
        if !ours(link) {
            return Ok(());
        }
        match std::fs::remove_file(link) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::PermissionDenied && cfg!(target_os = "macos") => {
                return as_admin(&format!("rm -f {}", sh(&link.display().to_string())));
            }
            Err(e) => return Err(format!("Couldn't remove {}: {e}", link.display())),
        }
        Ok(())
    }

    /// Only ever replace a link to AndruIDE, never someone else's file.
    fn refuse_others(link: &Path) -> Result<(), String> {
        match std::fs::symlink_metadata(link) {
            Ok(m) if !m.file_type().is_symlink() => Err(format!("There's already a file at {} that isn't a link to AndruIDE. Remove it first if you don't need it.", link.display())),
            Ok(_) if !std::fs::read_link(link).is_ok_and(|t| is_ours(&t)) => {
                Err(format!("{} already links to another program. Remove it first if you don't need it.", link.display()))
            }
            _ => Ok(()),
        }
    }

    fn link_here(link: &Path, exe: &Path) -> std::io::Result<()> {
        if let Some(dir) = link.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::symlink_metadata(link).is_ok() {
            std::fs::remove_file(link)?;
        }
        std::os::unix::fs::symlink(exe, link)
    }

    /// Runs a shell command as an administrator; macOS asks for the password.
    fn as_admin(command: &str) -> Result<(), String> {
        let applescript = format!("do shell script \"{}\" with administrator privileges", command.replace('\\', "\\\\").replace('"', "\\\""));
        let out = std::process::Command::new("osascript").arg("-e").arg(applescript).output().map_err(|e| format!("Couldn't ask for an administrator's password: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("-128") || err.to_lowercase().contains("cancel") {
            Err("Cancelled.".into())
        } else {
            Err(format!("It didn't work: {}", err.trim()))
        }
    }

    /// Single-quoted for sh.
    fn sh(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    // The user's PATH as stored (unexpanded, keeping its kind), changed in
    // place; SetEnvironmentVariable on a scratch name then tells Explorer and
    // new terminals to reload the environment.
    const READ: &str = r#"
$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
$p = if ($k) { $k.GetValue('Path', '', 'DoNotExpandEnvironmentNames') } else { '' }
[Console]::Out.Write($p)
"#;

    const WRITE: &str = r#"
$k = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$p = $k.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
$kind = if ($p) { $k.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
$parts = @($p -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $env:ANDRUIDE_DIR.TrimEnd('\')) })
if ($env:ANDRUIDE_ADD -eq '1') { $parts += $env:ANDRUIDE_DIR }
$k.SetValue('Path', ($parts -join ';'), $kind)
[Environment]::SetEnvironmentVariable('ANDRUIDE_PATH_CHANGED', $null, 'User')
"#;

    fn powershell(script: &str, env: &[(&str, &str)]) -> Result<String, String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]).creation_flags(CREATE_NO_WINDOW);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().map_err(|e| format!("Couldn't run PowerShell: {e}"))?;
        if !out.status.success() {
            return Err(format!("PowerShell: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn dir_of(exe: &Path) -> Result<String, String> {
        let dir = exe.parent().ok_or("Couldn't tell which folder AndruIDE is in.")?.display().to_string();
        // canonicalize gives \\?\C:\...; PATH wants C:\...
        Ok(dir.strip_prefix(r"\\?\").unwrap_or(&dir).to_string())
    }

    fn same(a: &str, b: &str) -> bool {
        a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
    }

    pub fn status(exe: &Path) -> Status {
        let dir = match dir_of(exe) {
            Ok(d) => d,
            Err(e) => return Status { state: "unsupported", note: Some(e), ..Default::default() },
        };
        let target = Some(exe.display().to_string().trim_start_matches(r"\\?\").to_string());
        match powershell(READ, &[]) {
            Ok(path) if path.split(';').any(|p| same(p, &dir)) => Status { state: "installed", path: dir, target, note: Some("Open a new terminal to use it.".into()), ..Default::default() },
            Ok(_) => Status { state: "missing", path: dir, target, ..Default::default() },
            Err(e) => Status { state: "unsupported", path: dir, target, note: Some(e), ..Default::default() },
        }
    }

    pub fn install(exe: &Path) -> Result<(), String> {
        powershell(WRITE, &[("ANDRUIDE_DIR", &dir_of(exe)?), ("ANDRUIDE_ADD", "1")]).map(|_| ())
    }

    pub fn remove(exe: &Path) -> Result<(), String> {
        powershell(WRITE, &[("ANDRUIDE_DIR", &dir_of(exe)?), ("ANDRUIDE_ADD", "0")]).map(|_| ())
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    #[test]
    fn knows_its_own_links() {
        assert!(is_ours(Path::new("/Applications/AndruIDE.app/Contents/MacOS/andruide")));
        assert!(is_ours(Path::new("/home/me/.cargo/bin/andruide")));
        assert!(is_ours(Path::new("/home/me/Applications/AndruIDE-0.5.2-linux-x86_64.AppImage")));
        assert!(!is_ours(Path::new("/usr/bin/python3")));
    }

    #[test]
    fn links_replaces_and_removes_only_its_own() {
        let tmp = std::env::temp_dir().join(format!("andruide-shell-command-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let exe = tmp.join("AndruIDE.app/Contents/MacOS/andruide");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        let exe = std::fs::canonicalize(&exe).unwrap();
        let link = tmp.join("bin/andruide");

        assert_eq!(imp::status_at(&link, &exe).state, "missing");
        imp::install_at(&link, &exe).unwrap();
        assert_eq!(imp::status_at(&link, &exe).state, "installed");
        // Again: replaces its own link.
        imp::install_at(&link, &exe).unwrap();
        assert_eq!(imp::status_at(&link, &exe).state, "installed");
        // A link to another copy is "other" and gets replaced.
        let old = tmp.join("Applications/AndruIDE.app/Contents/MacOS/andruide");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "").unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&old, &link).unwrap();
        assert_eq!((imp::status_at(&link, &exe).state, imp::status_at(&link, &exe).replaceable), ("other", true));
        imp::install_at(&link, &exe).unwrap();
        assert_eq!(imp::status_at(&link, &exe).state, "installed");
        imp::remove_at(&link).unwrap();
        assert_eq!(imp::status_at(&link, &exe).state, "missing");

        // Someone else's file, or a link to another program: left alone.
        std::fs::write(&link, "#!/bin/sh").unwrap();
        assert_eq!(imp::status_at(&link, &exe).state, "other");
        assert!(imp::install_at(&link, &exe).is_err());
        assert!(imp::remove_at(&link).is_err());
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink("/usr/bin/env", &link).unwrap();
        assert_eq!((imp::status_at(&link, &exe).state, imp::status_at(&link, &exe).replaceable), ("other", false));
        assert!(imp::install_at(&link, &exe).is_err());
        assert!(imp::remove_at(&link).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
