import CryptoKit
import Foundation
import Security

/// Asks GitHub for the newest TypeThru release. The request carries only the app name and
/// version (as the User-Agent). An update is downloaded and installed only when the user
/// chooses Install, and only if it is signed with the key in UpdateKey.swift.
enum Updates {
    struct Release: Equatable {
        let version: String
        let page: URL
        var download: URL? = nil
        var signature: URL? = nil
    }

    struct Failure: LocalizedError {
        let errorDescription: String?
        init(_ message: String) { errorDescription = message }
    }

    static let feed = URL(string: "https://api.github.com/repos/sayre4ux/TypeThru/releases?per_page=10")!

    static var currentVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    /// "v0.3.0-beta" → [0, 3, 0]
    static func numbers(_ version: String) -> [Int] {
        version.drop { !$0.isNumber }.prefix { $0.isNumber || $0 == "." }
            .split(separator: ".").compactMap { Int($0) }
    }

    static func isNewer(_ candidate: String, than current: String) -> Bool {
        let a = numbers(candidate), b = numbers(current)
        for index in 0..<max(a.count, b.count) {
            let x = index < a.count ? a[index] : 0, y = index < b.count ? b[index] : 0
            if x != y { return x > y }
        }
        return false
    }

    /// Only TypeThru's own release pages, so the Download button cannot open anything else.
    static func isReleasePage(_ page: URL) -> Bool {
        page.scheme == "https" && page.host == "github.com" && page.path.hasPrefix("/sayre4ux/TypeThru/releases/")
    }

    /// Only files attached to TypeThru's own releases.
    static func isReleaseFile(_ file: URL) -> Bool {
        file.scheme == "https" && file.host == "github.com" && file.path.hasPrefix("/sayre4ux/TypeThru/releases/download/")
    }

    /// The newest release in a GitHub API response. Only release pages of this project are
    /// accepted, so the Download button cannot be pointed anywhere else.
    static func newest(from data: Data) -> Release? {
        guard let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return nil }
        return list.compactMap { item -> Release? in
            guard item["draft"] as? Bool != true, let tag = item["tag_name"] as? String,
                  !numbers(tag).isEmpty, let link = item["html_url"] as? String, let page = URL(string: link),
                  isReleasePage(page) else { return nil }
            var release = Release(version: numbers(tag).map(String.init).joined(separator: "."), page: page)
            // The disk image and its signature, attached as TypeThru-<version>.dmg and .dmg.sig.
            var files: [String: URL] = [:]
            for asset in item["assets"] as? [[String: Any]] ?? [] {
                if let name = asset["name"] as? String, let link = asset["browser_download_url"] as? String,
                   let file = URL(string: link), isReleaseFile(file) { files[name] = file }
            }
            if let name = files.keys.sorted().first(where: { $0.hasSuffix(".dmg") && files[$0 + ".sig"] != nil }) {
                release.download = files[name]
                release.signature = files[name + ".sig"]
            }
            return release
        }
        .max { isNewer($1.version, than: $0.version) }
    }

    /// Calls back on the main thread with a newer release, nil when up to date, or an error.
    static func check(completion: @escaping (Result<Release?, Error>) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result { () -> Release? in
                let newest = newest(from: try fetch(feed, limit: 1 << 20, accept: "application/vnd.github+json"))
                return newest.flatMap { isNewer($0.version, than: currentVersion) ? $0 : nil }
            }
            DispatchQueue.main.async { completion(result) }
        }
    }

    /// Whether `signature` (base64 text, as in the .dmg.sig file) signs `data` with `key`.
    static func isSigned(_ data: Data, signature: Data, key: Data = UpdateKey.publicKey) -> Bool {
        guard let text = String(data: signature, encoding: .utf8),
              let raw = Data(base64Encoded: text.trimmingCharacters(in: .whitespacesAndNewlines)),
              let publicKey = try? Curve25519.Signing.PublicKey(rawRepresentation: key) else { return false }
        return publicKey.isValidSignature(raw, for: data)
    }

    /// Downloads `release`, checks it, and replaces the app at `app` with it. Blocks; call it
    /// off the main thread. The caller relaunches the app afterwards.
    static func install(_ release: Release, over app: URL = Bundle.main.bundleURL) throws {
        guard let download = release.download, let signatureFile = release.signature,
              isReleaseFile(download), isReleaseFile(signatureFile) else {
            throw Failure("This release has no signed download.")
        }
        let manager = FileManager.default
        guard manager.isWritableFile(atPath: app.deletingLastPathComponent().path) else {
            throw Failure("TypeThru cannot replace itself in \(app.deletingLastPathComponent().path).")
        }
        let signature = try fetch(signatureFile, limit: 1024)
        let image = try fetch(download, limit: 64 << 20)
        guard isSigned(image, signature: signature) else {
            throw Failure("The download is not signed by TypeThru, so it was not installed.")
        }
        // DECISION: a folder on the app's volume, so the final replace is a rename, not a copy.
        let work = try manager.url(for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: app, create: true)
        defer { try? manager.removeItem(at: work) }
        let disk = work.appendingPathComponent("update.dmg"), mount = work.appendingPathComponent("mount")
        let staged = work.appendingPathComponent(app.lastPathComponent)
        try image.write(to: disk)
        try manager.createDirectory(at: mount, withIntermediateDirectories: false)
        try run("/usr/bin/hdiutil", "attach", "-nobrowse", "-readonly", "-noautoopen", "-quiet", "-mountpoint", mount.path, disk.path)
        let copied = Result { try run("/usr/bin/ditto", mount.appendingPathComponent("TypeThru.app").path, staged.path) }
        try? run("/usr/bin/hdiutil", "detach", "-quiet", mount.path)
        try copied.get()
        try checkApp(at: staged)
        _ = try manager.replaceItemAt(app, withItemAt: staged)
    }

    /// The new app must be TypeThru, newer, and validly signed. Unless this copy is signed
    /// ad-hoc, it must also be signed by the same certificate, which keeps Accessibility allowed.
    private static func checkApp(at app: URL) throws {
        let bundle = Bundle(url: app)
        guard bundle?.bundleIdentifier == Bundle.main.bundleIdentifier,
              let version = bundle?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String,
              isNewer(version, than: currentVersion) else {
            throw Failure("The download is not a newer TypeThru.")
        }
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(app as CFURL, [], &code) == errSecSuccess, let code else {
            throw Failure("The new version's signature cannot be read.")
        }
        var requirement: SecRequirement?
        var me: SecCode?, myStatic: SecStaticCode?, info: CFDictionary?
        if SecCodeCopySelf([], &me) == errSecSuccess, let me, SecCodeCopyStaticCode(me, [], &myStatic) == errSecSuccess,
           let myStatic, SecCodeCopySigningInformation(myStatic, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess,
           let flags = (info as? [String: Any])?[kSecCodeInfoFlags as String] as? UInt32,
           flags & SecCodeSignatureFlags.adhoc.rawValue == 0 {
            _ = SecCodeCopyDesignatedRequirement(myStatic, [], &requirement)
        }
        let strict = SecCSFlags(rawValue: kSecCSCheckAllArchitectures | kSecCSStrictValidate | kSecCSCheckNestedCode)
        guard SecStaticCodeCheckValidity(code, strict, requirement) == errSecSuccess else {
            throw Failure(requirement == nil ? "The new version's signature is not valid."
                          : "The new version is signed by someone else. Download it from the release page instead.")
        }
    }

    // Ephemeral: no cookies or cache kept on disk.
    private static func fetch(_ url: URL, limit: Int, accept: String? = nil) throws -> Data {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 60)
        request.setValue("TypeThru/\(currentVersion)", forHTTPHeaderField: "User-Agent")
        if let accept { request.setValue(accept, forHTTPHeaderField: "Accept") }
        var result: Result<Data, Error> = .failure(URLError(.unknown))
        let done = DispatchSemaphore(value: 0)
        URLSession(configuration: .ephemeral).dataTask(with: request) { data, response, error in
            if let error { result = .failure(error) }
            else if let data, (response as? HTTPURLResponse)?.statusCode == 200, data.count <= limit { result = .success(data) }
            else { result = .failure(URLError(.badServerResponse)) }
            done.signal()
        }.resume()
        done.wait()
        return try result.get()
    }

    private static func run(_ tool: String, _ arguments: String...) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: tool)
        process.arguments = arguments
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw Failure("\(URL(fileURLWithPath: tool).lastPathComponent) failed while installing the update.")
        }
    }
}
