using System;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Reflection;
using System.Threading.Tasks;

namespace TypeThru;

/// <summary>
/// The only network access: the GitHub releases list, at most daily, and an update's files when
/// the user chooses Install. The request carries only the app name and version.
/// </summary>
internal static class Updater
{
    public static string CurrentVersion =>
        UpdateFeed.Normalized(Assembly.GetExecutingAssembly().GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion ?? "0");

    /// <summary>"0.7.0" reads as "0.7", as the releases are named.</summary>
    public static string Display(string version) =>
        version.Split('.').Length > 2 && version.EndsWith(".0", StringComparison.Ordinal) ? version[..^2] : version;

    private static HttpClient Client()
    {
        // No cookies or cache kept.
        var client = new HttpClient(new HttpClientHandler { UseCookies = false }) { Timeout = TimeSpan.FromSeconds(60) };
        client.DefaultRequestHeaders.UserAgent.ParseAdd($"TypeThru/{CurrentVersion}");
        return client;
    }

    private static async Task<byte[]> Fetch(HttpClient client, Uri url, long limit)
    {
        using var response = await client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead);
        response.EnsureSuccessStatusCode();
        if (response.Content.Headers.ContentLength > limit) throw new InvalidDataException("The download is larger than expected.");
        await using var stream = await response.Content.ReadAsStreamAsync();
        using var buffer = new MemoryStream();
        var chunk = new byte[81920];
        int read;
        while ((read = await stream.ReadAsync(chunk)) > 0)
        {
            buffer.Write(chunk, 0, read);
            if (buffer.Length > limit) throw new InvalidDataException("The download is larger than expected.");
        }
        return buffer.ToArray();
    }

    /// <summary>A newer release, or null when up to date.</summary>
    public static async Task<Release?> Check()
    {
        using var client = Client();
        client.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
        var json = System.Text.Encoding.UTF8.GetString(await Fetch(client, UpdateFeed.Feed, 1 << 20));
        return UpdateFeed.Newest(json, CurrentVersion);
    }

    /// <summary>
    /// Downloads <paramref name="release"/>, checks it, and puts it in place of this exe. The caller
    /// starts the new exe and quits. Trusts three things, all required: the file comes from a
    /// TypeThru release URL, its Ed25519 signature matches the built-in key, and it is a newer TypeThru.
    /// </summary>
    public static async Task Install(Release release)
    {
        if (release.Download is not { } download || release.Signature is not { } signatureFile
            || !UpdateFeed.IsReleaseFile(download) || !UpdateFeed.IsReleaseFile(signatureFile))
            throw new InvalidOperationException("This release has no signed Windows download.");
        var exe = Installation.CurrentExe;
        var folder = Path.GetDirectoryName(exe)!;
        var staged = exe + ".new";
        using (var client = Client())
        {
            var signature = await Fetch(client, signatureFile, 1024);
            var data = await Fetch(client, download, 300L << 20);
            if (!UpdateFeed.IsSigned(data, signature, UpdateFeed.PublicKey))
                throw new InvalidDataException("The download is not signed by TypeThru, so it was not installed.");
            try { await File.WriteAllBytesAsync(staged, data); }
            catch (UnauthorizedAccessException) { throw new InvalidOperationException($"TypeThru cannot replace itself in {folder}."); }
        }
        try
        {
            var info = FileVersionInfo.GetVersionInfo(staged);
            if (info.ProductName != "TypeThru" || !UpdateFeed.IsNewer(info.ProductVersion ?? "", CurrentVersion))
                throw new InvalidDataException("The download is not a newer TypeThru.");
            // Windows lets a running exe be renamed, not deleted: move it aside, then put the new one in place.
            File.Move(exe, exe + ".old", overwrite: true);
            try { File.Move(staged, exe); }
            catch (Exception) { File.Move(exe + ".old", exe); throw; }
        }
        finally { File.Delete(staged); }
    }

    /// <summary>Removes the copy an update moved aside, once it is no longer running.</summary>
    public static void CleanUp()
    {
        try { File.Delete(Installation.CurrentExe + ".old"); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
    }
}
