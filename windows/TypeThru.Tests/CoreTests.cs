using System.Text;
using System.Text.Json;
using Xunit;

namespace TypeThru.Tests;

/// <summary>The same rules as Tests/Tests.swift, plus the shared cases both apps must pass.</summary>
public class SharedCases
{
    private static readonly JsonElement Cases = JsonDocument.Parse(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "test-cases.json"))).RootElement;

    // A US keyboard: printable ASCII, Tab, and line breaks have keys.
    private static bool Typeable(string element) =>
        TextRules.IsLineBreak(element) || element == "\t" || (element.Length == 1 && element[0] is >= ' ' and <= '~');

    [Fact]
    public void Markdown()
    {
        foreach (var c in Cases.GetProperty("markdown").EnumerateArray())
        {
            var (text, formats, spans) = TextRules.Markdown(c.GetProperty("input").GetString()!);
            Assert.Equal(c.GetProperty("text").GetString(), text);
            var expected = c.GetProperty("formats").EnumerateObject().ToDictionary(
                p => int.Parse(p.Name, System.Globalization.CultureInfo.InvariantCulture),
                p => p.Value.EnumerateArray().Select(f => f.GetString() == "bold" ? TextFormat.Bold : TextFormat.Italic).ToList());
            Assert.Equal(expected.OrderBy(p => p.Key), formats.OrderBy(p => p.Key));
            Assert.True(spans > 0);
        }
    }

    [Fact]
    public void MarkdownUnchanged()
    {
        foreach (var c in Cases.GetProperty("markdownUnchanged").EnumerateArray())
        {
            var input = c.GetString()!;
            var (text, formats, spans) = TextRules.Markdown(input);
            Assert.Equal(input, text);
            Assert.Empty(formats);
            Assert.Equal(0, spans);
        }
    }

    [Fact]
    public void TrailingLineBreaks()
    {
        foreach (var c in Cases.GetProperty("trailingLineBreaks").EnumerateArray())
            Assert.Equal(c.GetProperty("output").GetString(), TextRules.WithoutTrailingLineBreaks(c.GetProperty("input").GetString()!));
    }

    [Fact]
    public void Replacements()
    {
        foreach (var c in Cases.GetProperty("replacements").EnumerateArray())
            Assert.Equal(c.GetProperty("output").GetString(), TextRules.ReplaceUntypable(c.GetProperty("input").GetString()!, Typeable).Text);
    }
}

public class TextRuleTests
{
    [Fact]
    public void LineBreakPairIsOneCharacter() =>
        Assert.Equal(["a", "\r\n", "b", "👨‍👩‍👧‍👦", "e\u0301"], TextRules.Elements("a\r\nb👨‍👩‍👧‍👦e\u0301"));

    [Fact]
    public void UnsupportedCharactersAreListedOnce() =>
        Assert.Equal(["中", "🙂"], TextRules.Unsupported("a中b中🙂", e => e.Length == 1 && e[0] < 128));

    [Fact]
    public void ReplacementOnlyWhenEveryCharacterHasAKey() =>
        Assert.Equal(("—", 0), TextRules.ReplaceUntypable("—", e => e == "a"));

    [Fact]
    public void EveryTypingTestIsTypeableOnUsKeyboard()
    {
        foreach (var test in Enum.GetValues<TypingTest>())
        {
            var text = TextRules.ReplaceUntypable(test.Text(), e => TextRules.IsLineBreak(e) || e == "\t" || (e.Length == 1 && e[0] is >= ' ' and <= '~')).Text;
            Assert.Empty(TextRules.Unsupported(text, e => TextRules.IsLineBreak(e) || e == "\t" || (e.Length == 1 && e[0] is >= ' ' and <= '~')));
        }
    }

    [Fact]
    public void Timing()
    {
        Assert.Equal((80, 160), Typing.Timing(0.24));
        Assert.Equal((10, 10), Typing.Timing(0.02));
        Assert.Equal((80, 500), Typing.Timing(double.PositiveInfinity));
        Assert.Equal((80, 160), Typing.Timing(double.NaN));
        Assert.Equal((10, 10), Typing.Timing(-5));
        Assert.Equal(300, Typing.SettleAfter("\r\n"));
        Assert.Equal(0, Typing.SettleAfter("a"));
    }

    [Fact]
    public void Shortcuts()
    {
        Assert.Null(Shortcut.Standard.Problem);
        Assert.Null(Shortcut.Standard.Caution);
        Assert.Null(Shortcut.Copy.Problem);
        Assert.NotEqual(Shortcut.Standard, Shortcut.Copy);
        Assert.NotNull(new Shortcut(Modifiers.Control, Shortcut.Escape).Problem);
        Assert.NotNull(new Shortcut(Modifiers.Shift, 0x41).Problem);
        Assert.Null(new Shortcut(Modifiers.None, Shortcut.F1 + 12).Problem);
        Assert.NotNull(new Shortcut(Modifiers.Control | Modifiers.Alt, 0x41).Caution);
    }
}

public class SettingsTests
{
    [Fact]
    public void RoundTrip()
    {
        var settings = new AppSettings { SpeedIndex = 3, Markdown = MarkdownMode.RemoveMarks, Method = TypingMethod.Unicode, LastRunVersion = "0.7" };
        var back = AppSettings.Parse(settings.Serialize());
        Assert.Equal(3, back.SpeedIndex);
        Assert.Equal(MarkdownMode.RemoveMarks, back.Markdown);
        Assert.Equal(TypingMethod.Unicode, back.Method);
        Assert.Equal("0.7", back.LastRunVersion);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("not json")]
    [InlineData("[1,2]")]
    [InlineData("{\"SpeedIndex\": 999, \"Method\": \"Nope\"}")]
    [InlineData("{\"SpeedIndex\": -4, \"ShortcutKey\": 27, \"ShortcutModifiers\": 2}")]
    [InlineData("{\"Markdown\": 77, \"ShortcutKey\": 65, \"ShortcutModifiers\": 0}")]
    public void DamagedFileGivesSafeValues(string? json)
    {
        var settings = AppSettings.Parse(json);
        Assert.InRange(settings.SpeedIndex, 0, Typing.Speeds.Count - 1);
        Assert.True(Enum.IsDefined(settings.Method));
        Assert.True(Enum.IsDefined(settings.Markdown));
        Assert.Null(settings.Shortcut.Problem);
    }
}

public class UpdateTests
{
    private static string Feed(params object[] items) => JsonSerializer.Serialize(items);

    private static object Item(string tag, string body = "## New\n- Something", bool draft = false, string? page = null, bool exe = true) => new Dictionary<string, object?>
    {
        ["tag_name"] = tag, ["draft"] = draft, ["body"] = body,
        ["html_url"] = page ?? $"https://github.com/sayre4ux/TypeThru/releases/tag/{tag}",
        ["assets"] = exe
            ? new[]
            {
                new { name = $"TypeThru-{tag}-windows.exe", browser_download_url = $"https://github.com/sayre4ux/TypeThru/releases/download/{tag}/TypeThru-{tag}-windows.exe" },
                new { name = $"TypeThru-{tag}-windows.exe.sig", browser_download_url = $"https://github.com/sayre4ux/TypeThru/releases/download/{tag}/TypeThru-{tag}-windows.exe.sig" },
                new { name = $"TypeThru-{tag}.dmg", browser_download_url = $"https://github.com/sayre4ux/TypeThru/releases/download/{tag}/TypeThru-{tag}.dmg" },
            }
            : [],
    };

    [Fact]
    public void Versions()
    {
        Assert.Equal([0, 3, 0], UpdateFeed.Numbers("v0.3.0-beta"));
        Assert.True(UpdateFeed.IsNewer("0.10", "0.9"));
        Assert.False(UpdateFeed.IsNewer("0.7.0", "0.7"));
        Assert.False(UpdateFeed.IsNewer("garbage", "0.1"));
    }

    [Fact]
    public void NewestWithWindowsDownload()
    {
        var release = UpdateFeed.Newest(Feed(Item("v0.7.0-beta"), Item("v0.6.1-beta")), "0.6.1")!;
        Assert.Equal("0.7.0", release.Version);
        Assert.EndsWith("-windows.exe", release.Download!.AbsolutePath);
        Assert.EndsWith("-windows.exe.sig", release.Signature!.AbsolutePath);
        Assert.Single(release.Notes);
    }

    [Fact]
    public void MacOnlyReleaseHasNoInstall()
    {
        var release = UpdateFeed.Newest(Feed(Item("v0.8", exe: false)), "0.7")!;
        Assert.Null(release.Download);
        Assert.True(UpdateFeed.IsReleasePage(release.Page));
    }

    [Fact]
    public void OnlyThisProjectAndNoDrafts()
    {
        Assert.Null(UpdateFeed.Newest(Feed(Item("v9.0", page: "https://example.com/sayre4ux/TypeThru/releases/tag/v9"), Item("v8.0", draft: true)), "0.7"));
        Assert.Null(UpdateFeed.Newest(Feed(Item("v0.7")), "0.7"));
        Assert.Null(UpdateFeed.Newest("not json", "0.1"));
        Assert.Null(UpdateFeed.Newest("{\"a\":1}", "0.1"));
    }

    [Fact]
    public void Notes()
    {
        var body = "Intro line.\n\n## New\n- **Copy Text** from screen\n- Bold\n\n## Fixes\n- No surprise commands\n\n## Install\n1. Download\n- not a change\n\nSHA-256: `abc`";
        Assert.Equal(["- **Copy Text** from screen", "- Bold", "- No surprise commands"], UpdateFeed.Summary(body));
        Assert.Equal(20, UpdateFeed.Summary(string.Concat(Enumerable.Repeat("- x\n", 50))).Count);
        Assert.Equal(400, UpdateFeed.Summary("- " + new string('y', 900))[0].Length);
        var notes = UpdateFeed.Notes(Feed(Item("v0.7.0-beta", "## New\n- Seven"), Item("v0.6.1-beta", "## Fixes\n- Settings"), Item("v0.6.0-beta"),
            Item("v0.6.2", "## Install\n- only steps")), "0.6");
        Assert.Equal(["0.7", "0.6.1"], notes.Select(n => n.Title));
    }

    [Fact]
    public void SignatureMatchesTheMacSigningScript()
    {
        // Same key and the signature format of scripts/update-key.swift: base64 text of a raw Ed25519 signature.
        var generator = new Org.BouncyCastle.Crypto.Generators.Ed25519KeyPairGenerator();
        generator.Init(new Org.BouncyCastle.Crypto.Parameters.Ed25519KeyGenerationParameters(new Org.BouncyCastle.Security.SecureRandom()));
        var pair = generator.GenerateKeyPair();
        var key = ((Org.BouncyCastle.Crypto.Parameters.Ed25519PublicKeyParameters)pair.Public).GetEncoded();
        var data = Encoding.UTF8.GetBytes("TypeThru update");
        var signer = new Org.BouncyCastle.Crypto.Signers.Ed25519Signer();
        signer.Init(true, pair.Private);
        signer.BlockUpdate(data, 0, data.Length);
        var signature = Encoding.UTF8.GetBytes(Convert.ToBase64String(signer.GenerateSignature()) + "\n");
        Assert.True(UpdateFeed.IsSigned(data, signature, key));
        Assert.False(UpdateFeed.IsSigned([.. data, 1], signature, key));
        Assert.False(UpdateFeed.IsSigned(data, signature, UpdateFeed.PublicKey));
        Assert.False(UpdateFeed.IsSigned(data, "not base64"u8.ToArray(), key));
        Assert.Equal(32, UpdateFeed.PublicKey.Length);
    }
}

public class NotesTests
{
    [Fact]
    public void BundledChangelog()
    {
        var entries = Changelog.Parse(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "CHANGELOG.md")));
        Assert.True(entries.Count >= 7);
        Assert.All(entries, e => Assert.All(e.Lines, l => Assert.StartsWith("- ", l)));
        Assert.Equal(["0.6.1", "0.6", "0.5.1", "0.5"], Changelog.Since("0.4", "0.6.1", entries).Select(e => e.Version));
        Assert.Empty(Changelog.Since("0.6", "0.6", entries));
    }

    [Fact]
    public void ContinuationLines() =>
        Assert.Equal([["- **a** b c"]], Changelog.Parse("## 1.2 · today\n- **a** b\n  c\n\n## no version\n- x").Select(e => e.Lines));

    [Fact]
    public void InlineStylesWithoutLinks()
    {
        var parts = InlineMarkdown.Parse("**Bold** and *it* with `**code**` and [a link](https://example.com).");
        Assert.Equal("Bold and it with **code** and a link.", string.Concat(parts.Select(p => p.Text)));
        Assert.Contains(parts, p => p.Text == "Bold" && p.Bold);
        Assert.Contains(parts, p => p.Text == "it" && p.Italic);
        Assert.Contains(parts, p => p.Text == "**code**" && p.Code && !p.Bold);
    }

    [Fact]
    public void ReadingOrder()
    {
        static Piece P(string t, double x, double y, double w = 300) => new(t, x, y, w, 40);
        // Pixel boxes, origin at the top left: an email with paragraph gaps.
        var email = new[] { P("Best regards,", 50, 800), P("Dear Chek,", 50, 100), P("Kyle", 50, 850), P("Noted the updated server reboot arrangements.", 50, 220, 800) };
        Assert.Equal("Dear Chek,\n\nNoted the updated server reboot arrangements.\n\nBest regards,\nKyle", ScreenLayout.Layout(email));
        var table = new[] { P("srv-db-01", 50, 500, 150), P("10.0.0.21", 550, 495, 150), P("Reboot", 300, 503, 100) };
        Assert.Equal("srv-db-01\tReboot\t10.0.0.21", ScreenLayout.Layout(table));
        Assert.Equal("Hello world", ScreenLayout.Layout([P("Hello", 100, 500, 100), P("world", 210, 500, 100)]));
        Assert.Equal("", ScreenLayout.Layout([]));
    }
}

/// <summary>Random input never crashes the rules, with a fixed seed so failures reproduce.</summary>
public class Fuzz
{
    private static readonly string[] Pieces = ["a", "Z", "9", " ", "\n", "\r\n", "\t", "'", "\"", "\\", "$", "*", "**", "***", "_", "•", "—",
        "“", "…", "\u2028", "\u200B", "\u00A0", "é", "e\u0301", "中", "🙂", "👨‍👩‍👧‍👦", "\0", "\u007F", "[", "](", ")", "`", "#", "- "];

    [Fact]
    public void TextRulesNeverCrash()
    {
        var random = new Random(0x54595045);
        for (var round = 0; round < 20000; round++)
        {
            var text = string.Concat(Enumerable.Range(0, random.Next(0, 24)).Select(_ => Pieces[random.Next(Pieces.Length)]));
            var (plain, formats, _) = TextRules.Markdown(text);
            var count = TextRules.Elements(plain).Count;
            Assert.All(formats.Keys, k => Assert.InRange(k, 0, count));
            // Every switch is undone: formatting never stays on after the text.
            Assert.All(Enum.GetValues<TextFormat>(), f => Assert.Equal(0, formats.Values.Sum(l => l.Count(x => x == f)) % 2));
            var trimmed = TextRules.Elements(TextRules.WithoutTrailingLineBreaks(text));
            Assert.True(trimmed.Count == 0 || !(TextRules.IsLineBreak(trimmed[^1]) || trimmed[^1] == "\u2028"));
            TextRules.ReplaceUntypable(text, e => e.Length == 1 && e[0] < 128);
            InlineMarkdown.Parse(text);
            Changelog.Parse(text);
            UpdateFeed.Summary(text);
            UpdateFeed.Newest(text, "0.1");
            AppSettings.Parse(text);
        }
    }
}
