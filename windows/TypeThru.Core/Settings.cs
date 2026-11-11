using System.Text.Json;
using System.Text.Json.Serialization;

namespace TypeThru;

public enum TypingMethod { ScanCodes, VirtualKeys, Unicode }

public static class TypingMethods
{
    public static string Title(this TypingMethod method) => method switch
    {
        TypingMethod.ScanCodes => "Hardware scan codes (recommended)",
        TypingMethod.VirtualKeys => "Virtual keys",
        _ => "Unicode text",
    };

    public static string Help(this TypingMethod method) => method switch
    {
        TypingMethod.ScanCodes => "Key presses as a physical keyboard sends them. VDI and remote desktop clients read these.",
        TypingMethod.VirtualKeys => "Windows key codes. Try this if scan codes give the wrong characters in a local app.",
        _ => "Characters instead of keys. Works in local apps for any character, but most remote sessions ignore it.",
    };
}

/// <summary>
/// Saved settings, as JSON in %APPDATA%\TypeThru\settings.json. Every value is checked on load,
/// so a damaged file gives defaults instead of a crash.
/// </summary>
public sealed class AppSettings
{
    public TypingMethod Method { get; set; } = TypingMethod.ScanCodes;
    public int SpeedIndex { get; set; }
    public bool ReplaceSymbols { get; set; } = true;
    // DECISION: on by default: Shift+Return makes a new line in chat apps and word processors
    // instead of sending. Spreadsheets move up a cell, so the panel can turn it off.
    public bool ShiftReturn { get; set; } = true;
    // DECISION: bold and italic by default; text from chat and AI tools uses Markdown.
    public MarkdownMode Markdown { get; set; } = MarkdownMode.Format;
    public bool CopyFromScreen { get; set; } = true;
    public bool CheckUpdates { get; set; } = true;
    public int ShortcutModifiers { get; set; } = (int)Shortcut.Standard.Modifiers;
    public int ShortcutKey { get; set; } = Shortcut.Standard.Key;
    public bool LaunchAtLoginAsked { get; set; }
    public bool InstallAsked { get; set; }
    public string? LastRunVersion { get; set; }
    public DateTimeOffset? LastUpdateCheck { get; set; }

    [JsonIgnore]
    public Speed Speed => Typing.Speeds[Math.Clamp(SpeedIndex, 0, Typing.Speeds.Count - 1)];

    [JsonIgnore]
    public Shortcut Shortcut
    {
        get
        {
            var shortcut = new Shortcut((Modifiers)(ShortcutModifiers & 0xF), ShortcutKey);
            return ShortcutKey is > 0 and < 0xFF && shortcut.Problem is null ? shortcut : Shortcut.Standard;
        }
        set { ShortcutModifiers = (int)value.Modifiers; ShortcutKey = value.Key; }
    }

    private static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,
        Converters = { new JsonStringEnumConverter() },
    };

    public static AppSettings Parse(string? json)
    {
        AppSettings settings;
        try { settings = (json is null ? null : JsonSerializer.Deserialize<AppSettings>(json, Options)) ?? new AppSettings(); }
        catch (Exception e) when (e is JsonException or NotSupportedException or ArgumentException) { settings = new AppSettings(); }
        if (!Enum.IsDefined(settings.Method)) settings.Method = TypingMethod.ScanCodes;
        if (!Enum.IsDefined(settings.Markdown)) settings.Markdown = MarkdownMode.Format;
        settings.SpeedIndex = Math.Clamp(settings.SpeedIndex, 0, Typing.Speeds.Count - 1);
        settings.Shortcut = settings.Shortcut;
        return settings;
    }

    public string Serialize() => JsonSerializer.Serialize(this, Options);
}
