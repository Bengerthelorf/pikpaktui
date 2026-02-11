mod backend;
mod native;
mod pikpak;
mod tui;

use crate::backend::Backend;
use crate::native::{
    NativeBackend, NativeBackendConfig,
    auth::{AuthConfig, NativeAuth, SessionToken},
};
use crate::pikpak::CliBackend;
use anyhow::Result;
use std::env;
use std::process::{Command, exit};

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

    if args.len() == 1 {
        let backend = select_backend()?;
        tui::run(backend)?;
        return Ok(());
    }

    let status = Command::new("pikpakcli")
        .args(&args[1..])
        .status()
        .expect("failed to execute pikpakcli");

    exit(status.code().unwrap_or(1));
}

fn select_backend() -> Result<Box<dyn Backend>> {
    let choice = env::var("PIKPAKTUI_BACKEND").unwrap_or_else(|_| "cli".into());
    if choice.eq_ignore_ascii_case("native") {
        Ok(Box::new(NativeBackend::new()?))
    } else {
        Ok(Box::new(CliBackend))
    }
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
        let (mut stream, _) = listener.accept().expect("accept failed");
        let mut buf = [0_u8; 4096];
        let _ = stream.read(&mut buf).expect("read failed");

        let body = r#"{"access_token":"tok-a","refresh_token":"tok-r","expires_in":3600}"#;
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

        let body = r#"{"files":[{"name":"Docs","kind":"folder"},{"name":"notes.txt","kind":"file","size":"12"}]}"#;
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
