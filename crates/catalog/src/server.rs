//! Starting and stopping the bundled Suwayomi process.

use crate::client::{Credentials, SuwayomiClient};
use crate::error::{CatalogError, Result};
use crate::provision::Install;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

const CONFIG_PREFIX: &str = "-Dsuwayomi.tachidesk.config.server";
const START_TIMEOUT: Duration = Duration::from_secs(180);
/// One readiness probe may take this long, so the start-up bound is real.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub install: Install,
    /// Suwayomi's data directory (database, extensions, downloads, logs).
    pub data_dir: PathBuf,
    /// Whether to allow the embedded Chromium. Off by default; see
    /// [`crate::browser`].
    pub browser_support: bool,
}

/// A running server plus a client already connected to it.
pub struct RunningServer {
    child: Child,
    pub client: SuwayomiClient,
    pub port: u16,
    pub log_path: PathBuf,
    pub data_dir: PathBuf,
}

impl RunningServer {
    /// Launch the server on a free loopback port, protected by a random
    /// password, and wait until it answers.
    pub async fn start(config: &ServerConfig) -> Result<Self> {
        let port = free_port()?;
        let credentials = Credentials {
            username: "mangapoint".into(),
            password: random_password()?,
        };
        std::fs::create_dir_all(config.data_dir.join("logs"))?;
        let log_path = config.data_dir.join("logs").join("mangapoint-server.log");
        let log = std::fs::File::create(&log_path)?;

        let mut cmd = Command::new(&config.install.java);
        cmd.args(java_args(config, port, &credentials))
            .current_dir(&config.install.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console flashing up behind the app.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn()?;

        let client = SuwayomiClient::new(format!("http://127.0.0.1:{port}"), Some(credentials))?;
        let waited = wait_until_ready(&client, START_TIMEOUT, PROBE_TIMEOUT, || {
            match child.try_wait()? {
                Some(status) => Err(CatalogError::ServerExited(
                    status.to_string(),
                    log_path.display().to_string(),
                )),
                None => Ok(()),
            }
        })
        .await;
        if let Err(e) = waited {
            let _ = child.kill().await;
            return Err(e);
        }
        Ok(Self {
            child,
            client,
            port,
            log_path,
            data_dir: config.data_dir.clone(),
        })
    }

    /// Stop the server. Suwayomi has no shutdown API, so this ends the
    /// process; its database tolerates that.
    pub async fn stop(mut self) -> Result<()> {
        self.child.kill().await?;
        Ok(())
    }

    /// Operating-system id of the server process, if it is still running.
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// Has the server process ended (crashed, killed, out of memory)?
    pub fn has_exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    /// Ask the OS to end the server without waiting. For use where `async`
    /// is not available, such as a window-close handler.
    pub fn kill_now(&mut self) {
        let _ = self.child.start_kill();
    }

    /// The last part of the server log, for progress and diagnostics.
    pub fn log_tail(&self, max_bytes: usize) -> String {
        read_tail(&self.log_path, max_bytes)
    }
}

/// Poll the server until it answers, `check_alive` reports it died, or
/// `total` has passed. Each probe is cut short at `probe` (and at what is
/// left of `total`), so a server that accepts but never answers cannot make
/// this run longer than `total`.
pub(crate) async fn wait_until_ready(
    client: &SuwayomiClient,
    total: Duration,
    probe: Duration,
    mut check_alive: impl FnMut() -> Result<()>,
) -> Result<()> {
    let started = Instant::now();
    loop {
        check_alive()?;
        let left = total.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err(CatalogError::StartTimeout(total));
        }
        let ping = client.graphql_within::<serde_json::Value>(
            "{ aboutServer { name version buildType } }",
            probe.min(left),
        );
        if ping.await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500).min(left)).await;
    }
}

fn java_args(config: &ServerConfig, port: u16, credentials: &Credentials) -> Vec<String> {
    let set = |key: &str, value: &str| format!("{CONFIG_PREFIX}.{key}={value}");
    vec![
        "--enable-native-access=ALL-UNNAMED".into(),
        set("rootDir", &config.data_dir.display().to_string()),
        // Loopback only: never reachable from the network.
        set("ip", "127.0.0.1"),
        set("port", &port.to_string()),
        // The app is the UI; no browser tab, tray icon or bundled web UI.
        set("initialOpenInBrowserEnabled", "false"),
        set("systemTrayEnabled", "false"),
        set("webUIEnabled", "false"),
        set("kcefEnabled", &config.browser_support.to_string()),
        // Without a password any local program or web page could drive it.
        set("authMode", "BASIC_AUTH"),
        set("authUsername", &credentials.username),
        set("authPassword", &credentials.password),
        "-jar".into(),
        config.install.jar.display().to_string(),
    ]
}

/// Ask the OS for an unused loopback port.
fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn random_password() -> Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|e| CatalogError::Io(std::io::Error::other(e.to_string())))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The last `max_bytes` of a log file; empty if it cannot be read.
pub fn read_tail(path: &Path, max_bytes: usize) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes as u64);
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).is_err() || f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(browser: bool) -> ServerConfig {
        ServerConfig {
            install: Install {
                dir: "/i".into(),
                java: "/i/jre/bin/java".into(),
                jar: "/i/bin/Suwayomi-Server.jar".into(),
            },
            data_dir: "/data".into(),
            browser_support: browser,
        }
    }

    fn creds() -> Credentials {
        Credentials {
            username: "u".into(),
            password: "p".into(),
        }
    }

    #[test]
    fn server_is_loopback_only_and_password_protected() {
        let args = java_args(&config(false), 4000, &creds());
        let has = |s: &str| args.iter().any(|a| a.ends_with(s));
        assert!(has("server.ip=127.0.0.1"));
        assert!(has("server.port=4000"));
        assert!(has("server.authMode=BASIC_AUTH"));
        assert!(has("server.webUIEnabled=false"));
        assert!(has("server.systemTrayEnabled=false"));
        assert_eq!(args[args.len() - 2], "-jar");
    }

    #[test]
    fn browser_support_follows_the_setting() {
        let off = java_args(&config(false), 1, &creds());
        let on = java_args(&config(true), 1, &creds());
        assert!(off.iter().any(|a| a.ends_with("kcefEnabled=false")));
        assert!(on.iter().any(|a| a.ends_with("kcefEnabled=true")));
    }

    #[tokio::test]
    async fn a_server_that_never_answers_still_times_out_on_schedule() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept and hold every connection without ever replying.
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((s, _)) = listener.accept().await {
                held.push(s);
            }
        });
        let client = SuwayomiClient::new(format!("http://127.0.0.1:{port}"), None).unwrap();
        let started = Instant::now();
        let err = wait_until_ready(
            &client,
            Duration::from_millis(900),
            Duration::from_millis(200),
            || Ok(()),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CatalogError::StartTimeout(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn waiting_stops_when_the_process_has_died() {
        let client = SuwayomiClient::new("http://127.0.0.1:1", None).unwrap();
        let err = wait_until_ready(&client, Duration::from_secs(30), Duration::from_secs(1), || {
            Err(CatalogError::ServerExited("1".into(), "log".into()))
        })
        .await
        .unwrap_err();
        assert!(matches!(err, CatalogError::ServerExited(..)));
    }

    #[test]
    fn passwords_are_random_and_long() {
        let a = random_password().unwrap();
        let b = random_password().unwrap();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
    }

    #[test]
    fn free_port_is_usable() {
        let p = free_port().unwrap();
        assert!(p > 0);
        assert!(std::net::TcpListener::bind(("127.0.0.1", p)).is_ok());
    }
}
