mod backend;
mod config;
mod native;
mod tui;

use crate::backend::Backend;
use crate::config::AppConfig;
use crate::native::{
    NativeBackend, NativeBackendConfig,
    auth::{AuthConfig, NativeAuth, SessionToken},
};
use anyhow::{Result, anyhow};
use std::env;
use std::process::exit;

fn main() {
    if let Err(e) = entry() {
        eprintln!("Error: {e:#}");
        exit(1);
    }
}

fn entry() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "--smoke-auth" {
        smoke_auth_roundtrip()?;
        return Ok(());
    }

    if args.len() > 1 && args[1] == "--smoke-native-login" {
        smoke_native_login()?;
        return Ok(());
    }

    if args.len() > 1 && args[1] == "--smoke-native-ls" {
        smoke_native_ls()?;
        return Ok(());
    }

    if args.len() > 1 && args[1] == "--smoke-native-ops" {
        smoke_native_ops()?;
        return Ok(());
    }

    if args.len() > 1 && args[1] == "--native-login" {
        native_login_from_env()?;
        return Ok(());
    }

    // 1. Try loading existing session
    let auth = NativeAuth::new()?;
    if let Ok(Some(session)) = auth.load_session() {
        if !session.is_expired(now_unix()) {
            // Valid session exists, go straight to TUI with backend
            let backend = NativeBackend::new()?;
            return tui::run_with_backend(Box::new(backend));
        }
    }

    // 2. No valid session — check config.yaml for credentials
    let cfg = AppConfig::load()?;
    let credentials = match (cfg.username, cfg.password) {
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => Some((u, p)),
        _ => None,
    };

    // 3. Launch TUI (with or without credentials)
    tui::run(credentials)?;
    Ok(())
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn native_login_from_env() -> Result<()> {
    let email = env::var("PIKPAK_EMAIL").map_err(|_| anyhow!("missing PIKPAK_EMAIL"))?;
    let password = env::var("PIKPAK_PASSWORD").map_err(|_| anyhow!("missing PIKPAK_PASSWORD"))?;

    let auth = NativeAuth::new()?;
    let token = auth.login_with_password(&email, &password)?;
    println!(
        "native-login-ok session={} access_len={}",
        auth.session_path().display(),
        token.access_token.len()
    );
    Ok(())
}

fn smoke_auth_roundtrip() -> Result<()> {
    let backend = NativeBackend::new()?;
    let auth = backend.auth();

    let token = SessionToken {
        access_token: "smoke-access".into(),
        refresh_token: "smoke-refresh".into(),
        expires_at_unix: 4_102_444_800,
    };

    auth.save_session(&token)?;
    let restored = auth.load_session()?.expect("session should exist");
    auth.clear_session()?;

    println!(
        "smoke-auth-ok path={} expires={}",
        auth.session_path().display(),
        restored.expires_at_unix
    );

    Ok(())
}

fn smoke_native_login() -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;

    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept failed");
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).expect("read failed");
            let req = String::from_utf8_lossy(&buf[..n]);

            let body = if req.starts_with("POST /v1/shield/captcha/init") {
                r#"{"captcha_token":"cap-1","url":"https://captcha.example"}"#
            } else {
                r#"{"access_token":"tok-a","refresh_token":"tok-r","expires_in":3600}"#
            };

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write failed");
        }
    });

    let auth = NativeAuth::from_config(AuthConfig {
        session_path: std::env::temp_dir().join("pikpaktui-smoke-login-session.json"),
        auth_base_url: format!("http://{}", addr),
        client_id: "smoke-client".into(),
        client_secret: "smoke-secret".into(),
    })?;

    let token = auth.login_with_password("smoke@example.com", "***")?;
    auth.clear_session()?;
    server.join().expect("server thread failed");

    println!(
        "smoke-native-login-ok access_len={} refresh_len={}",
        token.access_token.len(),
        token.refresh_token.len()
    );
    Ok(())
}

fn smoke_native_ls() -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept failed");
        let mut buf = [0_u8; 4096];
        let _ = stream.read(&mut buf).expect("read failed");

        let body = r#"{"files":[{"id":"d1","name":"Docs","kind":"folder"},{"id":"f1","name":"notes.txt","kind":"file","size":"12"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write failed");
    });

    let auth = NativeAuth::from_config(AuthConfig {
        session_path: std::env::temp_dir().join("pikpaktui-smoke-ls-session.json"),
        auth_base_url: "http://127.0.0.1:9".into(),
        client_id: "smoke-client".into(),
        client_secret: "smoke-secret".into(),
    })?;

    auth.save_session(&SessionToken {
        access_token: "tok-a".into(),
        refresh_token: "tok-r".into(),
        expires_at_unix: 4_102_444_800,
    })?;

    let backend = NativeBackend::from_config(NativeBackendConfig {
        auth,
        drive_base_url: format!("http://{}", addr),
    })?;

    let entries = backend.ls("/My Pack")?;
    backend.auth().clear_session()?;
    server.join().expect("server thread failed");

    println!("smoke-native-ls-ok count={}", entries.len());
    Ok(())
}

fn smoke_native_ops() -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;

    let server = thread::spawn(move || {
        for _ in 0..8 {
            let (mut stream, _) = listener.accept().expect("accept failed");
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).expect("read failed");
            let req = String::from_utf8_lossy(&buf[..n]);

            let body = if req.starts_with("GET /drive/v1/files") {
                r#"{"files":[{"id":"f1","name":"notes.txt","kind":"file","size":"12"}]}"#
            } else {
                "{}"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write failed");
        }
    });

    let auth = NativeAuth::from_config(AuthConfig {
        session_path: std::env::temp_dir().join("pikpaktui-smoke-ops-session.json"),
        auth_base_url: "http://127.0.0.1:9".into(),
        client_id: "smoke-client".into(),
        client_secret: "smoke-secret".into(),
    })?;

    auth.save_session(&SessionToken {
        access_token: "tok-a".into(),
        refresh_token: "tok-r".into(),
        expires_at_unix: 4_102_444_800,
    })?;

    let backend = NativeBackend::from_config(NativeBackendConfig {
        auth,
        drive_base_url: format!("http://{}", addr),
    })?;

    backend.mv("/My Pack", "notes.txt", "/Archive")?;
    backend.rename("/My Pack", "notes.txt", "notes-2.txt")?;
    backend.remove("/My Pack", "notes.txt")?;
    backend.cp("/My Pack", "notes.txt", "/Backup")?;

    backend.auth().clear_session()?;
    server.join().expect("server thread failed");

    println!("smoke-native-ops-ok");
    Ok(())
}
