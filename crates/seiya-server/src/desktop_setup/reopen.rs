//! La segunda apertura recupera la página normal, nunca el enlace de autenticación.
use crate::{local_models::storage, settings_menu};
use std::{fs, os::unix::fs::FileTypeExt, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

fn path() -> Result<PathBuf, String> {
    let root = storage::default_root()?;
    storage::ensure_directory(&root)?;
    Ok(root.join("desktop-8015.sock"))
}

pub(super) struct Owner(PathBuf, tokio::task::JoinHandle<()>);

impl Drop for Owner {
    fn drop(&mut self) {
        self.1.abort();
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn serve(home: String) -> Result<Owner, String> {
    let path = path()?;
    // Solo el proceso que ya ha adquirido el puerto 8015 llega aquí.
    if let Ok(m) = fs::symlink_metadata(&path) {
        if !m.file_type().is_socket() {
            return Err("desktop_reopen_path_busy".into());
        }
        fs::remove_file(&path).map_err(|_| "desktop_reopen_cleanup")?;
    }
    let listener = UnixListener::bind(&path).map_err(|_| "desktop_reopen_bind")?;
    Ok(Owner(
        path,
        tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                let _ = client.write_all(home.as_bytes()).await;
            }
        }),
    ))
}

pub(super) async fn current() -> Result<String, String> {
    let path = path()?;
    tokio::time::timeout(Duration::from_secs(2), async {
        let socket = UnixStream::connect(path).await.ok()?;
        if socket.peer_cred().ok()?.uid() != unsafe { libc::geteuid() } {
            return None;
        }
        let mut home = String::new();
        socket.take(256).read_to_string(&mut home).await.ok()?;
        (settings_menu::desktop_port(&home).ok() == Some(8015)).then_some(home)
    })
    .await
    .ok()
    .flatten()
    .ok_or_else(|| "desktop_port_busy_no_adoption".into())
}
