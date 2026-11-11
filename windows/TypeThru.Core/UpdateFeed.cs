using System.Text.Json;
using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Crypto.Signers;

namespace TypeThru;

public sealed record ChangelogEntry(string Version, string Title, IReadOnlyList<string> Lines);

public sealed record Release(string Version, Uri Page)
{
    public Uri? Download { get; init; }
    public Uri? Signature { get; init; }
    /// <summary>What changed in every release after this copy, newest first, shown before Install.</summary>
    public IReadOnlyList<ChangelogEntry> Notes { get; init; } = [];
}

/// <summary>
/// Reads the GitHub releases list, as the Mac app does (Sources/Updates.swift). The request is
/// made by Updater in the app; this part only reads the answer, so it is tested on any OS.
/// </summary>
public static class UpdateFeed
{
    public static readonly Uri Feed = new("https://api.github.com/repos/sayre4ux/TypeThru/releases?per_page=10");
    public const string AssetSuffix = "-windows.exe";

    /// <summary>"v0.3.0-beta" → [0, 3, 0]</summary>
    public static int[] Numbers(string version)
    {
        var start = version.SkipWhile(c => !char.IsAsciiDigit(c));
        var digits = new string(start.TakeWhile(c => char.IsAsciiDigit(c) || c == '.').ToArray());
        return digits.Split('.', StringSplitOptions.RemoveEmptyEntries)
            .Select(p => int.TryParse(p, out var v) ? v : -1).Where(v => v >= 0).ToArray();
    }

    public static string Normalized(string version) => string.Join('.', Numbers(version));

    public static bool IsNewer(string candidate, string current)
    {
        var a = Numbers(candidate);
        var b = Numbers(current);
        for (var i = 0; i < Math.Max(a.Length, b.Length); i++)
        {
            var x = i < a.Length ? a[i] : 0;
            var y = i < b.Length ? b[i] : 0;
            if (x != y) return x > y;
        }
        return false;
    }

    /// <summary>Only TypeThru's own release pages, so a button cannot open anything else.</summary>
    public static bool IsReleasePage(Uri page) =>
        page.Scheme == "https" && page.Host == "github.com" && page.AbsolutePath.StartsWith("/sayre4ux/TypeThru/releases/", StringComparison.Ordinal);

    /// <summary>Only files attached to TypeThru's own releases.</summary>
    public static bool IsReleaseFile(Uri file) =>
        file.Scheme == "https" && file.Host == "github.com" && file.AbsolutePath.StartsWith("/sayre4ux/TypeThru/releases/download/", StringComparison.Ordinal);

    private static List<JsonElement> Items(string json)
    {
        try
        {
            using var document = JsonDocument.Parse(json);
            return document.RootElement.ValueKind == JsonValueKind.Array
                ? document.RootElement.EnumerateArray().Where(e => e.ValueKind == JsonValueKind.Object).Select(e => e.Clone()).ToList()
                : [];
        }
        catch (JsonException) { return []; }
    }

    private static string? Text(JsonElement item, string name) =>
        item.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;

    private static bool Draft(JsonElement item) =>
        item.TryGetProperty("draft", out var value) && value.ValueKind == JsonValueKind.True;

    /// <summary>The newest release of this project, with the Windows download and its signature when attached.</summary>
    public static Release? Newest(string json, string current)
    {
        Release? newest = null;
        foreach (var item in Items(json))
        {
            if (Draft(item) || Text(item, "tag_name") is not { } tag || Numbers(tag).Length == 0
                || !Uri.TryCreate(Text(item, "html_url"), UriKind.Absolute, out var page) || !IsReleasePage(page)) continue;
            var release = new Release(Normalized(tag), page);
            var files = new Dictionary<string, Uri>();
            if (item.TryGetProperty("assets", out var assets) && assets.ValueKind == JsonValueKind.Array)
            {
                foreach (var asset in assets.EnumerateArray())
                {
                    if (asset.ValueKind == JsonValueKind.Object && Text(asset, "name") is { } name
                        && Uri.TryCreate(Text(asset, "browser_download_url"), UriKind.Absolute, out var file) && IsReleaseFile(file))
                        files[name] = file;
                }
            }
            var exe = files.Keys.Order(StringComparer.Ordinal).FirstOrDefault(n => n.EndsWith(AssetSuffix, StringComparison.Ordinal) && files.ContainsKey(n + ".sig"));
            if (exe is not null) release = release with { Download = files[exe], Signature = files[exe + ".sig"] };
            if (newest is null || IsNewer(release.Version, newest.Version)) newest = release;
        }
        return newest is not null && IsNewer(newest.Version, current) ? newest with { Notes = Notes(json, current) } : null;
    }

    /// <summary>
    /// The notes of every release after <paramref name="current"/>, newest first. Only the bullets
    /// under New, Fixes, and Security; install steps and checksums are left out.
    /// </summary>
    public static List<ChangelogEntry> Notes(string json, string current)
    {
        var entries = new List<ChangelogEntry>();
        foreach (var item in Items(json))
        {
            if (Draft(item) || Text(item, "tag_name") is not { } tag || Text(item, "body") is not { } body
                || !Uri.TryCreate(Text(item, "html_url"), UriKind.Absolute, out var page) || !IsReleasePage(page)) continue;
            var version = Normalized(tag);
            if (version.Length == 0 || !IsNewer(version, current)) continue;
            var lines = Summary(body);
            // "0.7.0" reads as "0.7", as the releases are named.
            var title = version.Split('.').Length > 2 && version.EndsWith(".0", StringComparison.Ordinal) ? version[..^2] : version;
            if (lines.Count > 0) entries.Add(new ChangelogEntry(version, title, lines));
        }
        entries.Sort((a, b) => IsNewer(a.Version, b.Version) ? -1 : IsNewer(b.Version, a.Version) ? 1 : 0);
        return entries.Take(10).ToList();
    }

    public static List<string> Summary(string body)
    {
        var keep = true;
        var lines = new List<string>();
        foreach (var raw in body.Split('\n'))
        {
            var line = raw.Trim();
            if (line.StartsWith('#'))
            {
                var heading = line.TrimStart('#').Trim().ToLowerInvariant();
                keep = new[] { "new", "what's new", "fixes", "security", "changes" }.Any(h => heading.StartsWith(h, StringComparison.Ordinal));
            }
            else if (keep && line.StartsWith("- ", StringComparison.Ordinal))
            {
                lines.Add(line.Length > 400 ? line[..400] : line);
            }
        }
        return lines.Take(20).ToList();
    }

    /// <summary>Whether <paramref name="signature"/> (base64 text, as in the .sig file) signs <paramref name="data"/> with <paramref name="key"/>.</summary>
    public static bool IsSigned(byte[] data, byte[] signature, byte[] key)
    {
        try
        {
            var raw = Convert.FromBase64String(System.Text.Encoding.UTF8.GetString(signature).Trim());
            if (raw.Length != 64 || key.Length != 32) return false;
            var verifier = new Ed25519Signer();
            verifier.Init(false, new Ed25519PublicKeyParameters(key, 0));
            verifier.BlockUpdate(data, 0, data.Length);
            return verifier.VerifySignature(raw);
        }
        catch (FormatException) { return false; }
    }

    /// <summary>The public key built into both apps; the private key is in the release signer's keychain.</summary>
    public static readonly byte[] PublicKey = Convert.FromBase64String("BXnQoapwfgyibBMFwhRrwwvxzlQxg8124OvklxqxoAI="); // gitleaks:allow (public key)
}
