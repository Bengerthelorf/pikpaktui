// The CLI and JNI compile the same protocol modules and expose different subsets.
#[allow(dead_code, unused_imports)]
#[path = "../../src/pikpak/mod.rs"]
mod pikpak;

mod folder_scans;
use folder_scans::{FolderScans, SCANS};

use anyhow::{Context, Result, anyhow, bail, ensure};
use jni::JNIEnv;
use jni::objects::{JObject, JString};
use jni::sys::jstring;
use pikpak::{
    CreateShareResponse, DownloadControl, LoginSessionSaveError, OFFLINE_TASK_PHASES, PikPak,
    RefreshError, SignInRequired, UploadControl, interactive,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::Duration;

static CLIENT: RwLock<Option<Arc<PikPak>>> = RwLock::new(None);
static TRANSFERS: LazyLock<Mutex<HashMap<String, Arc<NativeTransfer>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
enum NativeTransfer {
    Download(DownloadControl),
    Upload(UploadControl),
}

fn transfer_control(args: &Value) -> Result<Arc<NativeTransfer>> {
    let key = required(args, "transfer")?;
    TRANSFERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow!("transfer is no longer active"))
}

fn transfer_request(op: &str, args: &Value) -> Result<Value> {
    let key = required(args, "transfer")?;
    match op {
        "transfer_begin" => {
            let control = match required(args, "kind")? {
                "download" => NativeTransfer::Download(DownloadControl::default()),
                "upload" => NativeTransfer::Upload(UploadControl::default()),
                _ => bail!("unknown transfer kind"),
            };
            let mut transfers = TRANSFERS.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(
                !transfers.contains_key(key),
                "transfer is already registered"
            );
            transfers.insert(key.to_owned(), Arc::new(control));
        }
        "transfer_stop" => match &*transfer_control(args)? {
            NativeTransfer::Download(control) => control.stop(),
            NativeTransfer::Upload(control) => control.cancel(),
        },
        "transfer_progress" => {
            return Ok(match &*transfer_control(args)? {
                NativeTransfer::Download(control) => {
                    let (bytes, total) = control.progress();
                    json!({"bytes": bytes, "total": total})
                }
                NativeTransfer::Upload(control) => {
                    let progress = control.snapshot();
                    json!({"bytes": progress.bytes, "total": progress.total, "stage": progress.stage})
                }
            });
        }
        "transfer_end" => {
            TRANSFERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(key);
        }
        _ => bail!("unknown transfer command"),
    }
    Ok(Value::Null)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_snaix_pikpak_NativeClient_request(
    mut env: JNIEnv<'_>,
    _object: JObject<'_>,
    request: JString<'_>,
) -> jstring {
    let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let request: String = env.get_string(&request)?.into();
        dispatch(&serde_json::from_str(&request)?)
    }));
    let envelope = match reply {
        Ok(Ok(value)) => json!({"value": value}),
        Ok(Err(error)) => error_payload(error),
        Err(_) => json!({"error": "PikPak core error", "code": "internal"}),
    };
    env.new_string(envelope.to_string())
        .map(JString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

fn error_payload(error: anyhow::Error) -> Value {
    if matches!(
        error.downcast_ref(),
        Some(pikpak::FolderStatsError::Cancelled)
    ) {
        return json!({"error": "Folder scan cancelled", "code": "cancelled"});
    }
    if error.downcast_ref::<pikpak::UnsupportedIndex>().is_some() {
        return json!({"error": "This account or drive listing does not support the persistent index", "code": "index_unavailable"});
    }
    let progress = pikpak::mutation_progress(&error);
    if let Some(challenge) = error.downcast_ref::<pikpak::LoginChallenge>() {
        return json!({"error": "PikPak requires verification", "code": "challenge", "challenge": challenge.0});
    }
    if error.downcast_ref::<LoginSessionSaveError>().is_some() {
        return json!({"error": "Could not save the login session on this device", "code": "session_save"});
    }
    let message = format!("{error:#}");
    let lower = message.to_lowercase();
    let refresh_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<RefreshError>());
    let (code, text) = if progress.pending || progress.accepted || progress.completed_items > 0 {
        (
            "pending",
            "The operation may have reached PikPak. Refresh the affected folder or task list before retrying.",
        )
    } else if matches!(refresh_error, Some(RefreshError::Unavailable)) {
        (
            "unavailable",
            "PikPak is temporarily unavailable. Try again shortly.",
        )
    } else if matches!(refresh_error, Some(RefreshError::Rejected))
        || error.chain().any(|cause| cause.is::<SignInRequired>())
    {
        ("auth", "Sign in to PikPak again.")
    } else if lower.contains("signin failed") {
        (
            "login",
            "PikPak rejected sign-in. Check your email and password.",
        )
    } else if lower.contains("file_restore_own") || lower.contains("already belong") {
        ("own_share", "This share belongs to your account.")
    } else if lower.contains("share is not available") || lower.contains("pass_code") {
        ("share", "Check the share link and access code.")
    } else if lower.contains("quota_exceeded")
        || lower.contains("quota_limit")
        || lower.contains("space_not_enough")
    {
        ("quota", "PikPak reported a quota limit.")
    } else if error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut)
    }) {
        ("timeout", "PikPak took too long to respond.")
    } else if let Some(http) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<reqwest::Error>())
    {
        if http.is_timeout() {
            ("timeout", "PikPak took too long to respond.")
        } else if http.is_connect() {
            ("connect", "The connection to PikPak failed.")
        } else {
            ("network", "The connection to PikPak failed.")
        }
    } else if lower.contains("risklimited") || lower.contains("captcha") || lower.contains("403") {
        (
            "restricted",
            "PikPak restricted this request. Verify the account on PikPak and try again.",
        )
    } else {
        ("request", "PikPak could not complete this request.")
    };
    json!({"error": text, "code": code})
}

fn dispatch(args: &Value) -> Result<Value> {
    let op = required(args, "op")?;
    match op {
        "core_info" => {
            return Ok(json!({
                "api": 1,
                "upstream": env!("PIKPAKTUI_REVISION"),
                "version": env!("PIKPAKTUI_VERSION"),
                "source": env!("PIKPAKTUI_SOURCE"),
                "capabilities": ["folder_index", "drive_search_cached"],
            }));
        }
        "folder_stats_begin" => {
            let key = required(args, "scan")?;
            SCANS.lock().unwrap_or_else(|e| e.into_inner()).begin(
                key,
                args["root"].as_bool().unwrap_or(false),
                args["fresh"].as_bool().unwrap_or(false),
            )?;
            return Ok(Value::Null);
        }
        "folder_stats_cancel" => {
            let key = required(args, "scan")?;
            SCANS.lock().unwrap_or_else(|e| e.into_inner()).cancel(key);
            return Ok(Value::Null);
        }
        "drive_search_cached" => {
            let client = interactive::read(&CLIENT)?.clone().ok_or(SignInRequired)?;
            let query = serde_json::from_value::<pikpak::MobileSearchQuery>(args.clone())
                .context("invalid cached search query")?;
            let page = client.mobile_search_cached(&query)?;
            check_active_client(&client)?;
            return encode(page);
        }
        "drive_search_clear" => {
            let client = interactive::read(&CLIENT)?.clone().ok_or(SignInRequired)?;
            client.clear_mobile_search_cache()?;
            check_active_client(&client)?;
            return Ok(Value::Null);
        }
        "folder_stats_cached" => {
            let ids: Vec<String> = serde_json::from_value(args["ids"].clone())
                .context("expected a list of folder IDs")?;
            let guard = interactive::read(&CLIENT)?;
            let client = guard.as_ref().ok_or(SignInRequired)?;
            let found = client.cached_folder_stats_batch(&ids)?;
            let mut result = serde_json::Map::new();
            for id in ids {
                let value = found.get(&id).map(stats_reply).unwrap_or(Value::Null);
                result.insert(id, value);
            }
            return Ok(Value::Object(result));
        }
        "folder_index_sync" => {
            let key = required(args, "scan")?;
            let scan = SCANS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .registered(key)?;
            let client = interactive::read(&CLIENT)?.clone().ok_or(SignInRequired)?;
            FolderScans::sync_index(
                &SCANS,
                &client,
                &scan,
                args["force_full"].as_bool().unwrap_or(false),
            )
            .map_err(|error| {
                // API v1 clients released before indexed search use "request" to
                // trigger scoped folder reads when a watermark is unavailable.
                if error.is::<pikpak::UnsupportedIndex>()
                    && args["report_unavailable"].as_bool() != Some(true)
                {
                    anyhow!(error.to_string())
                } else {
                    error
                }
            })?;
            check_active_client(&client)?;
            return Ok(Value::Null);
        }
        "folder_stats" => {
            let id = args["id"].as_str().context("missing folder id")?;
            let scan = if args.get("scan").is_some() {
                let key = required(args, "scan")?;
                Some(
                    SCANS
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .registered(key)?,
                )
            } else {
                None
            };
            let client = interactive::read(&CLIENT)?.clone().ok_or(SignInRequired)?;
            let stats = FolderScans::stats(&SCANS, &client, id, scan.as_deref())?;
            check_active_client(&client)?;
            return Ok(stats_reply(&stats));
        }
        "transfer_begin" | "transfer_stop" | "transfer_progress" | "transfer_end" => {
            return transfer_request(op, args);
        }
        "init" => {
            return interactive::with_deadline(Duration::from_secs(5), || {
                initialize_session(PathBuf::from(required(args, "path")?))
            });
        }
        "login" => {
            return interactive::with_deadline(Duration::from_secs(30), || {
                let path = PathBuf::from(required(args, "path")?);
                let mut guard = interactive::write(&CLIENT)?;
                let mut client = PikPak::with_session_path(path)?;
                client.login_with_challenge_token(
                    required(args, "email")?,
                    required(args, "password")?,
                    args["challenge_token"].as_str(),
                )?;
                replace_client(&mut guard, Some(client));
                Ok(Value::Null)
            });
        }
        "logout" => {
            return interactive::with_deadline(Duration::from_secs(5), || {
                let path = PathBuf::from(required(args, "path")?);
                let mut guard = interactive::write(&CLIENT)?;
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                replace_client(&mut guard, None);
                Ok(Value::Null)
            });
        }
        "hash" => return encode(pikpak::pikpak_hash(Path::new(required(args, "path")?))?),
        _ => {}
    }

    let perform = || {
        let guard = interactive::read(&CLIENT)?;
        let client = guard.as_ref().ok_or(SignInRequired)?;
        call(client, op, args)
    };
    // Directory browsing and previews have interactive deadlines. Transfers keep the
    // core's normal longer request timeout, including its range/resume checks.
    match op {
        "files" | "info" | "text" | "links" | "trash" | "starred" | "account" | "events"
        | "tasks" | "shares" | "share_open" | "share_folder" => {
            interactive::with_deadline(Duration::from_secs(24), perform)
        }
        _ => perform(),
    }
}

// A login may already have replaced the saved session while holding the
// writer lock, just before it publishes the new client and cancels old scans.
fn check_active_client(client: &Arc<PikPak>) -> Result<()> {
    let guard = interactive::read(&CLIENT)?;
    ensure!(
        guard
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, client)),
        pikpak::FolderStatsError::Cancelled
    );
    Ok(())
}

fn stats_reply(stats: &pikpak::FolderStats) -> Value {
    json!({
        "bytes": stats.total_size_bytes.to_string(),
        "modified_at": stats.latest_modified_time,
        "is_empty": stats.is_empty,
    })
}

fn replace_client(slot: &mut Option<Arc<PikPak>>, next: Option<PikPak>) {
    let mut scans = SCANS.lock().unwrap_or_else(|e| e.into_inner());
    *slot = next.map(Arc::new);
    scans.change_account(slot.as_ref());
}

fn initialize_session(path: PathBuf) -> Result<Value> {
    {
        let guard = interactive::read(&CLIENT)?;
        if let Some(client) = guard
            .as_ref()
            .filter(|client| client.uses_session_path(&path))
        {
            return Ok(json!(client.load_session()?.is_some()));
        }
    }
    let mut guard = interactive::write(&CLIENT)?;
    if guard
        .as_ref()
        .is_none_or(|client| !client.uses_session_path(&path))
    {
        let client = PikPak::with_session_path(path)?;
        replace_client(&mut guard, Some(client));
    }
    Ok(json!(
        guard
            .as_ref()
            .ok_or(SignInRequired)?
            .load_session()?
            .is_some()
    ))
}

fn call(client: &PikPak, op: &str, args: &Value) -> Result<Value> {
    let id = || required(args, "id");
    let parent = || args["parent"].as_str().unwrap_or("");
    let ids = || ids(args);
    let name = || required(args, "name");
    let result = match op {
        "files" => encode(FolderScans::files(&SCANS, client, parent())?)?,
        "starred" => encode(client.starred_list(500)?)?,
        "trash" => encode(client.ls_trash(500)?)?,
        "info" => encode(client.file_info(id()?)?)?,
        "links" => {
            let info = client.file_info(id()?)?;
            let play: Vec<_> = info
                .play_options()
                .into_iter()
                .map(|option| {
                    json!({
                        "label": option.display_label(),
                        "url": option.url,
                        "available": option.available,
                        "reason": option.unavailable_reason,
                    })
                })
                .collect();
            json!({
                "download": info.download_url(),
                "thumbnail": info.thumbnail_link,
                "size": info.file_size(),
                "mime": info.mime_type,
                "play": play,
            })
        }
        "text" => {
            let (_, content, _, truncated) = client.fetch_text_preview(id()?, 128 * 1024)?;
            json!({"text": content, "truncated": truncated})
        }
        "mkdir" => encode(client.mkdir(parent(), name()?)?)?,
        "rename" => {
            client.rename(id()?, name()?)?;
            Value::Null
        }
        "move" => {
            client.mv(&ids()?, parent())?;
            Value::Null
        }
        "copy" => {
            client.cp(&ids()?, parent())?;
            Value::Null
        }
        "star" => {
            client.star(&ids()?)?;
            Value::Null
        }
        "unstar" => {
            client.unstar(&ids()?)?;
            Value::Null
        }
        "remove" => {
            client.remove(&ids()?)?;
            Value::Null
        }
        "restore" => {
            client.untrash(&ids()?)?;
            Value::Null
        }
        "destroy" => {
            client.delete_permanent(&ids()?)?;
            Value::Null
        }
        "empty_trash" => {
            client.empty_trash()?;
            Value::Null
        }
        "offline" => {
            let link = required(args, "url")?;
            validate_resource_link(link)?;
            let name = args["name"].as_str().filter(|name| !name.is_empty());
            encode(client.offline_download(link, args["parent"].as_str(), name)?)?
        }
        "tasks" => encode(client.offline_list(500, OFFLINE_TASK_PHASES)?.tasks)?,
        "retry_task" => {
            client.offline_task_retry(id()?)?;
            Value::Null
        }
        "delete_tasks" => {
            client.delete_tasks(&ids()?, args["delete_files"].as_bool().unwrap_or(false))?;
            Value::Null
        }
        "account" => {
            let quota = client.quota()?.quota;
            json!({
                "quota": quota,
                "vip": client.vip_info().ok().and_then(|value| value.data),
                "bandwidth": client.transfer_quota().ok().and_then(|value| value.base),
                "user": client.user_info().ok(),
            })
        }
        "events" => encode(client.events(100)?.events)?,
        "shares" => encode(client.list_shares()?)?,
        "share_create" => {
            let days = args["days"].as_i64().unwrap_or(-1);
            ensure!(matches!(days, -1 | 1 | 7 | 30), "invalid expiration");
            let ids = ids()?;
            let protected = args["protected"].as_bool().unwrap_or(false);
            if args["separate"].as_bool().unwrap_or(false) {
                create_separate_shares(&ids, |file_id| {
                    client.create_share(&[file_id], protected, days)
                })?
            } else {
                encode(client.create_share(&ids, protected, days)?)?
            }
        }
        "share_delete" => {
            client.delete_shares(&ids()?)?;
            Value::Null
        }
        "share_open" => {
            let url = required(args, "url")?;
            let location = share_location(url)?;
            let code = args["code"]
                .as_str()
                .filter(|value| !value.is_empty())
                .or(location.pass_code.as_deref())
                .unwrap_or("");
            let info = client.share_info(&location.id, code)?;
            location.breadcrumbs(client, &info.pass_code_token)?;
            let files =
                client.share_detail(&location.id, location.folder(None)?, &info.pass_code_token)?;
            // The existing app passes this handle back verbatim on refresh and navigation.
            // A deep link anchors its empty-parent view at the opened folder across calls.
            let handle = share_handle(url, &location)?;
            json!({"share": handle, "token": info.pass_code_token, "files": files})
        }
        "share_folder" => {
            let location = share_location(required(args, "share")?)?;
            let folder = if parent().is_empty() {
                location.folder(None)?
            } else {
                parent()
            };
            encode(client.share_detail(
                &location.id,
                folder,
                args["token"].as_str().unwrap_or(""),
            )?)?
        }
        "share_preview" => {
            let location = share_location(required(args, "share")?)?;
            encode(client.share_preview(
                &location.id,
                id()?,
                args["token"].as_str().unwrap_or(""),
            )?)?
        }
        "share_save" => {
            let location = share_location(required(args, "share")?)?;
            client.save_share(
                &location.id,
                args["token"].as_str().unwrap_or(""),
                &ids()?,
                parent(),
            )?;
            Value::Null
        }
        "download" => {
            let path = Path::new(required(args, "path")?);
            if args["transfer"].is_string() {
                let control = transfer_control(args)?;
                let NativeTransfer::Download(control) = &*control else {
                    bail!("wrong transfer kind");
                };
                encode(client.download_to_controlled(id()?, path, control)?)?
            } else {
                encode(client.download_to(id()?, path)?)?
            }
        }
        "download_folder" => {
            let (done, failed) =
                client.download_dir(id()?, name()?, Path::new(required(args, "path")?), 3)?;
            json!({"done": done, "failed": failed})
        }
        "upload" => {
            let path = Path::new(required(args, "path")?);
            if args["transfer"].is_string() {
                let control = transfer_control(args)?;
                let NativeTransfer::Upload(control) = &*control else {
                    bail!("wrong transfer kind");
                };
                client.upload_path_controlled(parent(), path, control)?;
                Value::Null
            } else {
                let (id, instant) = client.upload_file(Some(parent()), path)?;
                json!({"id": id, "instant": instant})
            }
        }
        _ => bail!("unknown operation"),
    };
    Ok(result)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .with_context(|| format!("missing {key}"))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn ids(args: &Value) -> Result<Vec<&str>> {
    let list = args["ids"]
        .as_array()
        .filter(|values| !values.is_empty())
        .context("select files first")?;
    list.iter()
        .map(|value| {
            value
                .as_str()
                .filter(|id| valid_id(id))
                .context("invalid ID")
        })
        .collect()
}

fn encode(value: impl serde::Serialize) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

fn create_separate_shares(
    ids: &[&str],
    mut create: impl FnMut(&str) -> Result<CreateShareResponse>,
) -> Result<Value> {
    let mut shares = Vec::with_capacity(ids.len());
    for id in ids {
        match create(id) {
            Ok(response) => shares.push(json!({ "file_id": id, "share": response })),
            Err(error) if shares.is_empty() => return Err(error),
            Err(_) => break,
        }
    }
    Ok(json!({ "shares": shares }))
}

fn validate_resource_link(input: &str) -> Result<()> {
    let url = reqwest::Url::parse(input)?;
    match url.scheme() {
        "magnet" => ensure!(
            url.query_pairs()
                .any(|(key, value)| key == "xt" && value.starts_with("urn:btih:")),
            "invalid magnet link"
        ),
        "http" | "https" => ensure!(url.host_str().is_some(), "invalid URL"),
        _ => bail!("unsupported download link"),
    }
    Ok(())
}

fn share_location(input: &str) -> Result<pikpak::ShareLocation> {
    let location = pikpak::ShareLocation::parse(input)?;
    if !valid_id(input.trim()) {
        let url = reqwest::Url::parse(input.trim())?;
        ensure!(
            matches!(
                url.host_str(),
                Some("mypikpak.com" | "www.mypikpak.com" | "share.mypikpak.com")
            ),
            "invalid PikPak share link"
        );
    }
    Ok(location)
}

fn share_handle(input: &str, location: &pikpak::ShareLocation) -> Result<String> {
    if location.folders.is_empty() {
        return Ok(location.id.clone());
    }
    let url = reqwest::Url::parse(input.trim())?;
    Ok(format!("https://mypikpak.com{}", url.path()))
}

#[cfg(test)]
mod tests {
    use super::{CreateShareResponse, create_separate_shares, share_handle, share_location};

    #[test]
    fn cancelled_folder_scan_stops_workers_and_rejects_queued_scans() {
        use super::{SCANS, dispatch, error_payload, json};
        use std::sync::atomic::Ordering;

        let token = "folder-scan-test";
        let begin = json!({"op": "folder_stats_begin", "scan": token});
        let cancel = json!({"op": "folder_stats_cancel", "scan": token});
        let queued = json!({"op": "folder_stats", "id": "", "scan": token});
        dispatch(&begin).unwrap();
        let worker = SCANS.lock().unwrap().registered(token).unwrap();
        assert!(dispatch(&begin).is_err());
        assert!(!worker.cancelled.load(Ordering::Relaxed));
        dispatch(&cancel).unwrap();
        dispatch(&cancel).unwrap();
        assert!(worker.cancelled.load(Ordering::Relaxed));
        assert_eq!(
            error_payload(dispatch(&queued).unwrap_err())["code"],
            "cancelled"
        );
    }

    #[test]
    fn separate_shares_keep_confirmed_results_without_repeating_mutations() {
        let mut attempts = Vec::new();
        let result = create_separate_shares(&["first", "next", "unattempted"], |id| {
            attempts.push(id.to_owned());
            anyhow::ensure!(id == "first", "fixture failure");
            Ok(CreateShareResponse {
                share_id: "fixture".into(),
                share_url: "https://mypikpak.com/s/fixture".into(),
                pass_code: String::new(),
                share_text: String::new(),
            })
        })
        .unwrap();
        assert_eq!(attempts, ["first", "next"]);
        assert_eq!(result["shares"].as_array().unwrap().len(), 1);
        assert_eq!(result["shares"][0]["file_id"], "first");
        assert_eq!(
            result["shares"][0]["share"]["share_url"],
            "https://mypikpak.com/s/fixture"
        );
    }

    #[test]
    fn share_selection_never_expands_a_deep_link() {
        let root = "https://mypikpak.com/s/test_Id/?pass_code=a%2Bb";
        let loc = share_location(root).unwrap();
        assert_eq!(loc.id, "test_Id");
        assert_eq!(loc.pass_code.as_deref(), Some("a+b"));
        assert_eq!(share_handle(root, &loc).unwrap(), "test_Id");
        let deep = "https://www.mypikpak.com/s/VOIQ0QFrIMiPE8EFPZYJKKxpo1/AAAeZ9plmSBi6NG9KffhrhCAo1_VOI?pass_code=private#fragment";
        let loc = share_location(deep).unwrap();
        let handle = share_handle(deep, &loc).unwrap();
        assert!(!handle.contains("private") && !handle.contains('#'));
        let reopened = share_location(&handle).unwrap();
        assert_eq!(reopened.folder(None).unwrap(), "VOIOttsOuejt-xC4FGHohrzpo1");
        assert_eq!(reopened.id, loc.id);
        assert!(share_location("https://mypikpak.com/s/test_Id/child").is_err());
        assert!(share_location("https://mypikpak.com/s/test_Id/child/..").is_err());
        assert!(share_location("https://example.com/s/test_Id").is_err());
    }
}
