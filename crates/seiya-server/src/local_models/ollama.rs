//! File discovery only; never executes binaries or opens runtime/model data.
use std::path::{Path, PathBuf};

pub(super) fn candidates(
    os: &str,
    home: Option<&str>,
    local: Option<&str>,
    programs: Option<&str>,
) -> Vec<PathBuf> {
    match os {
        "macos" => vec![
            "/Applications/Ollama.app/Contents/Resources/ollama".into(),
            "/usr/local/bin/ollama".into(),
            "/opt/homebrew/bin/ollama".into(),
            "/Applications/Ollama.app/Contents/MacOS/ollama".into(),
        ],
        "linux" => {
            let mut paths = vec!["/usr/local/bin/ollama".into(), "/usr/bin/ollama".into()];
            if let Some(home) = home {
                paths.push(Path::new(home).join(".local/bin/ollama"));
            }
            paths
        }
        "windows" => {
            let mut paths = vec![];
            if let Some(local) = local {
                paths.push(
                    format!(
                        r"{}\Programs\Ollama\ollama.exe",
                        local.trim_end_matches('\\')
                    )
                    .into(),
                );
            }
            if let Some(programs) = programs {
                paths
                    .push(format!(r"{}\Ollama\ollama.exe", programs.trim_end_matches('\\')).into());
            }
            paths
        }
        _ => vec![],
    }
}
pub(super) fn detect(
    paths: &[PathBuf],
    saved: Option<&str>,
    accessible: impl Fn(&Path) -> bool,
) -> (String, String) {
    if let Some(path) = saved.filter(|s| !s.is_empty()) {
        let message = if accessible(Path::new(path)) {
            "Ollama encontrado en la ruta guardada."
        } else {
            "Ruta guardada de Ollama inválida o no accesible. Cámbiala en Configuración; no se usa otra ruta automáticamente."
        };
        return (path.into(), message.into());
    }
    if let Some(path) = paths.iter().find(|p| accessible(p)) {
        return (
            path.to_string_lossy().into_owned(),
            "Ollama encontrado en una ruta habitual.".into(),
        );
    }
    (paths.first().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), "Ollama no encontrado en las rutas habituales. Puede estar instalado en otra ubicación; revisa la ruta en Configuración.".into())
}
pub(crate) fn native(saved: Option<&str>) -> (String, String) {
    let paths = candidates(
        crate::platform::current().os.name(),
        std::env::var("HOME").ok().as_deref(),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
        std::env::var("ProgramFiles").ok().as_deref(),
    );
    detect(&paths, saved, accessible)
}
fn accessible(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    std::fs::File::open(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_paths_and_overrides_do_not_fallback() {
        let mac = candidates("macos", None, None, None);
        assert_eq!(mac.len(), 4);
        let linux = candidates("linux", Some("/synthetic/user"), None, None);
        assert_eq!(linux[2], PathBuf::from("/synthetic/user/.local/bin/ollama"));
        let windows = candidates(
            "windows",
            None,
            Some(r"C:\Users\fixture\AppData\Local"),
            Some(r"C:\Program Files"),
        );
        assert_eq!(
            windows[1],
            PathBuf::from(r"C:\Program Files\Ollama\ollama.exe")
        );
        for paths in [mac, linux, windows] {
            let (path, status) = detect(&paths, None, |p| p == paths[1]);
            assert_eq!(path, paths[1].to_string_lossy());
            assert!(status.contains("encontrado"));
            let (path, status) = detect(&paths, Some("custom-invalid"), |_| false);
            assert_eq!(path, "custom-invalid");
            assert!(status.contains("inválida"));
            let (path, status) = detect(&paths, None, |_| false);
            assert_eq!(path, paths[0].to_string_lossy());
            assert!(status.contains("no encontrado"));
            assert!(detect(&paths, Some("custom-valid"), |p| p
                == Path::new("custom-valid"))
            .1
            .contains("ruta guardada"));
        }
    }
    #[test]
    fn directories_missing_and_non_executable_files_are_not_found() {
        let root = std::env::temp_dir().join(format!("seiya-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        assert!(!accessible(&root));
        assert!(!accessible(&root.join("missing")));
        let file = root.join("ollama");
        std::fs::write(&file, b"synthetic; never executed").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(!accessible(&file));
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(accessible(&file));
        std::fs::remove_dir_all(root).unwrap();
    }
}
