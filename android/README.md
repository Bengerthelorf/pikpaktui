# Android JNI API v1

The arm64 library implements `com.snaix.pikpak.NativeClient.request(String): String` for Android API level 26 and later. This crate compiles the repository's `src/pikpak` modules directly. The JNI target selects its session, challenge and transfer adapters at compile time. The desktop package keeps its own configuration and proactive refresh policy.

## Build and test

Run these commands from a Git checkout, with Rust 1.91 and Android NDK r27d on a Linux or macOS host. Point `ANDROID_NDK_HOME` to the unpacked NDK. Cargo honors `CARGO_HOME`, `CARGO_TARGET_DIR` and `TMPDIR`.

```sh
rustup target add aarch64-linux-android
cargo test --locked --manifest-path android/Cargo.toml
scripts/build-android-core.sh dist
```

The script checks the ELF target, JNI symbol and 16 KiB LOAD segment alignment before copying the raw library. To exercise the Java signature and JSON envelopes on a host with JDK 17+ and Python 3.11+, run `cargo build --locked --manifest-path android/Cargo.toml`, then `python3 android/tests/jni_smoke.py android/target/debug/libpikpak_android.so`. Run `python3 android/tests/folder_stats.py android/target/debug/libpikpak_android.so` to check network requests, fallback and cancellation against a local fixture. Use the host's `.dylib` suffix on macOS or the equivalent path under `CARGO_TARGET_DIR`.

## Release asset

Select the exact asset `pikpaktui-android-arm64-v8a-api1.so` from the [latest formal release](https://api.github.com/repos/Bengerthelorf/pikpaktui/releases/latest). Download the raw bytes over HTTPS from its `browser_download_url` and check their SHA-256 against the asset's `digest` field (`sha256:<hex>`) before loading. The same digest is listed in `sha256sums.txt`. [GitHub release asset fields](https://docs.github.com/en/rest/releases/assets#get-a-release-asset)

Load the verified absolute path with `System.load`. A call with `{"op":"core_info"}` returns a `value` object with integer `api`, the 40-character `upstream` Git commit, the root pikpaktui crate `version`, and `source`. GitHub builds check `GITHUB_SHA` against the checkout. Local builds report `source: "local-worktree"` and the base HEAD, which can have uncommitted changes. The ABI version in the filename is independent of the Android OS API level.

### Maintainer release

Merge the JNI sources, shared protocol changes, tests and workflow into `main`. After CI passes, start from a clean `main` synchronized with `origin/main` and run `./scripts/release.sh X.Y.Z` with the next unused version. The script updates the desktop crate version and lockfile, tests, then pushes the release commit and its tag atomically.

The Android job builds and checks the raw ELF. The tag workflow packages it alongside the desktop binaries, writes `sha256sums.txt` and creates the release. Verify the fixed asset name and the matching SHA-256 in the Releases API and checksum file. Droid can then download the library from that release. The existing crates.io and Homebrew jobs follow the release.

## Requests and replies

A request is a JSON object with an `op` string and operation arguments. Success returns `{"value": ...}`, including `null` for commands. Failures contain `error` and `code`. A login challenge also contains the `challenge` URL. Clients may ignore additional fields in an API v1 response.

| Operations | API v1 behavior |
| --- | --- |
| `core_info`, `init`, `login`, `logout` | `init(path)` reports whether the stored session exists. Complete interactive verification by repeating `login` with the same credentials and `challenge_token`. |
| `files`, `starred`, `trash`, `info`, `links`, `text` | List entries, inspect a file, obtain stream and download URLs, or return a bounded text preview. |
| `folder_stats_begin`, `folder_stats`, `folder_stats_cancel` | Register a caller-generated `scan` key, query by folder `id` (empty for root), then release the key with cancel. A query may omit `scan`. Results contain decimal-string `bytes`, nullable `modified_at` and boolean `is_empty`. Cancelled scans return code `cancelled`. |
| `folder_stats_cached`, `folder_index_sync` | Available when `core_info.value.capabilities` contains `"folder_index"`. Batch-read the last committed folder totals offline, or synchronize the current account's index using a registered scan. |
| `drive_search_cached`, `drive_search_clear` | Available when `core_info.value.capabilities` contains `"drive_search_cached"`. Read a page of filename matches from the current account’s committed index or clear its cached search data. |
| `mkdir`, `rename`, `move`, `copy`, `star`, `unstar`, `remove`, `restore`, `destroy`, `empty_trash` | Drive mutations. Code `pending` indicates a mutation may have been accepted, so refresh state before retrying. |
| `offline`, `tasks`, `retry_task`, `delete_tasks`, `account`, `events` | Submit and manage offline tasks, read account information and events. |
| `shares`, `share_create`, `share_delete`, `share_open`, `share_folder`, `share_preview`, `share_save` | Manage shares, browse root and nested links, preview shared file metadata, and restore selected IDs. Pass the `share` and `token` returned by `share_open` to subsequent operations; an empty source parent reopens the same linked folder. |
| `download`, `download_folder`, `upload`, `hash` | A file download returns bytes, folder download returns `done` and `failed`, and ordinary upload returns `id` and `instant`. Controlled upload returns `null`. Hash returns the GCID. |
| `transfer_begin`, `transfer_progress`, `transfer_stop`, `transfer_end` | Register `transfer` and `kind` (`upload` or `download`), pass the key to the transfer, and query or stop it concurrently. End releases the key. Downloads can resume from their partial file. Progress contains `bytes` and `total`, with `stage` on upload. |

### Folder scan hints

Call ordinary `files` to obtain the current directory. Pass optional booleans on `folder_stats_begin`, for example `{"op":"folder_stats_begin","scan":"view-42","root":true,"fresh":true}`. Use `root` for the actual cloud-drive root and `fresh` for an explicit user refresh. Both default to false. Registration and cancellation use local state. Run the subsequent `folder_stats` calls on background workers.

On an account with a saved user ID, the first statistics query on a registration synchronizes the persistent folder index. A cold index needs one paginated whole-drive listing, including when the first view is a nested directory. Later registrations fetch changes since the last completed page. The core commits that page’s server timestamp with the index and overlaps one second on the next query, preserving the server’s timestamp precision and time zone. A successful full reconciliation is due again after 72 hours on the next synchronization, to catch permanent removals the service omitted from its changes. An unusable checkpoint can also require a new full listing. All folders on the same view share this synchronization. At the actual root the core also checks the result against the normal listing already displayed. Ordinary `files` calls continue to fetch and authenticate each listing.

If synchronization fails, existing `folder_stats` callers use the scoped traversal. If the session has no stable account identity the existing in-memory statistics path remains available, including its checked cold-root optimization. A `folder_stats` call without a registration retains its original behavior. Cancel a registration when leaving the view. Explicit refresh cancels earlier registrations, clears the in-memory traversal cache and keeps the last committed durable index for incremental synchronization. Account changes isolate the durable results and invalidate old registrations. Local drive mutations invalidate registrations belonging to the previous directory generation.

### Reading the persistent index

1. Check `core_info` for the optional `folder_index` capability. On a worker, call `{"op":"folder_stats_cached","ids":["","folder-id"]}`. The response maps each requested ID to its last committed statistics or `null` on a miss. The empty string identifies the drive root. A completed empty folder returns `{"bytes":"0","modified_at":null,"is_empty":true}`. This read uses no network, including after an app restart. A hit can predate changes made on another device.
2. To update the snapshot, register a scan and run `{"op":"folder_index_sync","scan":"view-42"}` on a worker. Success returns `{"value":null}`. Batch-read the same IDs again to update the display. Errors and cancellation leave the previous committed snapshot available. The existing `folder_stats_cancel` operation cancels the sync.
3. A manual refresh uses the same incremental synchronization. Pass `"force_full":true` to `folder_index_sync` when deliberately rebuilding the index from a full listing. Use a new registration for the next incremental refresh.

### Searching the persistent index

Call `drive_search_cached` on a worker to search by case-insensitive filename substring. For example:

```json
{"op":"drive_search_cached","query":"三上","sort":"Recent","descending":true,"folder_stats":false,"limit":300}
```

A `null` value means the account has no complete search snapshot. A page with `count: 0` means a complete snapshot was searched and had no matches. Reads use no network. A page contains:

- `checked_at`, the Unix time in seconds of the last successful index check.
- `count`, the total number of matches across all pages, and `has_next`, which indicates more results.
- `files`, a page of drive entries in the requested order. Entries include `id`, `name`, `parent_id`, `kind`, size, timestamps, star state and thumbnail URL when available. With `folder_stats: true`, matching folders also carry their committed totals.

Choose `Name`, `Created`, `Recent` (modified time), `ContentModified`, `Kind` or `Size` and a page size from 1 to 300. Ordering applies to all matches before the core selects the page. To continue, pass the entire last row as `after` with the same query and sorting options. Start from the first page again after synchronization or a change of sort.

A client can display the first page immediately, refresh a stale index with `folder_index_sync`, then read the first page again. Search and folder statistics share that sync. An older on-disk index can continue serving folder statistics while search awaits its first metadata backfill. Pass `"report_unavailable":true` to `folder_index_sync` to receive the specific code `index_unavailable` for a missing stable account ID or server watermark. Omitting it preserves the API v1 `request` code for that operation. Clients can then use their older listing-based search. A temporary network failure leaves the last committed search snapshot available. Handle the usual `auth` code through the client’s sign-in flow.

Use `{"op":"drive_search_clear"}` to clear the current account’s search cache. A subsequent search awaits a fresh baseline. Previously committed folder statistics remain available.
