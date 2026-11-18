//! The only network access (the Mac `Updates`): the GitHub releases list, at most daily, and an
//! update's files when the user chooses Install. Uses Windows' own HTTP stack (WinHTTP), so no
//! TLS library is built in. The request carries only the app name and version.

use std::ffi::c_void;
use std::path::PathBuf;
use typethru_core::update_feed::{self, FEED, Release, is_newer, is_release_file, normalized};
use url::Url;
use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_COOKIES, WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

pub fn current_version() -> String {
    normalized(env!("CARGO_PKG_VERSION"))
}

/// Closes a WinHTTP handle when dropped.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from WinHTTP and is closed once.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn opened(handle: *mut c_void) -> Result<Handle, String> {
    if handle.is_null() { Err(windows::core::Error::from_thread().message()) } else { Ok(Handle(handle)) }
}

/// Downloads `url` over HTTPS, up to `limit` bytes. Redirects are followed, never to plain HTTP.
fn fetch(url: &Url, limit: usize, accept: Option<&str>) -> Result<Vec<u8>, String> {
    let host = url.host_str().filter(|_| url.scheme() == "https").ok_or("Only HTTPS downloads are allowed.")?;
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let agent = HSTRING::from(format!("TypeThru/{}", current_version()));
    // SAFETY: every handle is checked and closed by `Handle`; strings and buffers live until the
    // call that takes them returns; WinHttpReadData writes at most the buffer's length.
    unsafe {
        let session = opened(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        let _ = WinHttpSetTimeouts(session.0, 30_000, 30_000, 30_000, 60_000);
        let protocols = (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3).to_le_bytes();
        let _ = WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&protocols));
        let connection = opened(WinHttpConnect(session.0, &HSTRING::from(host), url.port().unwrap_or(443), 0))?;
        let request = opened(WinHttpOpenRequest(
            connection.0,
            &HSTRING::from("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        // No cookies kept.
        let _ = WinHttpSetOption(Some(request.0), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_COOKIES.to_le_bytes()));
        let headers: Option<Vec<u16>> = accept.map(|a| format!("Accept: {a}\r\n").encode_utf16().collect());
        let fail = |e: windows::core::Error| e.message();
        WinHttpSendRequest(request.0, headers.as_deref(), None, 0, 0, 0).map_err(fail)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(fail)?;
        let (mut status, mut size) = (0u32, size_of::<u32>() as u32);
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut status).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(fail)?;
        if status != 200 {
            return Err(format!("The server answered {status}."));
        }
        let mut data = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut read).map_err(fail)?;
            if read == 0 {
                return Ok(data);
            }
            data.extend_from_slice(&chunk[..read as usize]);
            if data.len() > limit {
                return Err("The download is larger than expected.".into());
            }
        }
    }
}

/// A newer release, or None when up to date. Blocks; run it on a worker thread.
pub fn check() -> Result<Option<Release>, String> {
    let feed = Url::parse(FEED).map_err(|e| e.to_string())?;
    let json = fetch(&feed, 1 << 20, Some("application/vnd.github+json"))?;
    Ok(update_feed::newest(&String::from_utf8_lossy(&json), &current_version()))
}

fn staged(exe: &std::path::Path, suffix: &str) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Downloads `release`, checks it, and puts it in place of this exe; the caller starts the new exe
/// and quits. Trusts three things, all required: the file comes from a TypeThru release URL, its
/// Ed25519 signature matches the built-in key, and it is a newer TypeThru. Blocks. Returns the
/// path of the new exe (where this one was).
pub fn install(release: &Release) -> Result<PathBuf, String> {
    let (Some(download), Some(signature_file)) = (&release.download, &release.signature) else {
        return Err("This release has no signed Windows download.".into());
    };
    if !is_release_file(download) || !is_release_file(signature_file) {
        return Err("This release has no signed Windows download.".into());
    }
    let exe = crate::install::current_exe();
    let folder = exe.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let signature = fetch(signature_file, 1024, None)?;
    let data = fetch(download, 300 << 20, None)?;
    if !update_feed::is_signed(&data, &signature, &update_feed::public_key()) {
        return Err("The download is not signed by TypeThru, so it was not installed.".into());
    }
    let (new, old) = (staged(&exe, ".new"), staged(&exe, ".old"));
    std::fs::write(&new, &data).map_err(|_| format!("TypeThru cannot replace itself in {folder}."))?;
    let result = (|| {
        let newer =
            crate::native::product_of(&new).is_some_and(|(name, version)| name == "TypeThru" && is_newer(&normalized(&version), &current_version()));
        if !newer {
            return Err("The download is not a newer TypeThru.".to_owned());
        }
        // Windows lets a running exe be renamed, not deleted: move it aside, then put the new one in place.
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|_| format!("TypeThru cannot replace itself in {folder}."))?;
        if std::fs::rename(&new, &exe).is_err() {
            let _ = std::fs::rename(&old, &exe);
            return Err(format!("TypeThru cannot replace itself in {folder}."));
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&new);
    result.map(|()| exe)
}

/// Removes the copy an update moved aside, once it is no longer running.
pub fn clean_up() {
    let _ = std::fs::remove_file(staged(&crate::install::current_exe(), ".old"));
}
