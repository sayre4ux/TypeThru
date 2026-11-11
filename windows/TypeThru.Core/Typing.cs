namespace TypeThru;

public sealed record Speed(string Title, double Interval, string Detail);

/// <summary>Speeds, key timing, and typing tests, the same as the Mac app.</summary>
public static class Typing
{
    public static readonly IReadOnlyList<Speed> Speeds =
    [
        new("Average typist · 50 WPM", 0.24, "240 ms per key. A typical typist, and the most reliable."),
        new("Fast typist · 100 WPM", 0.12, "120 ms per key. The fastest 5% of typists."),
        new("Record typist · 200 WPM", 0.06, "60 ms per key. World-record pace."),
        new("Superhuman · 400 WPM", 0.03, "30 ms per key. Some remote sessions drop keys."),
        new("Unrealistic · 600 WPM", 0.02, "20 ms per key. Expect dropped keys on slow connections."),
    ];

    /// <summary>
    /// How long a key is held and the pause after it, in milliseconds. A damaged saved setting
    /// (huge, infinite, or NaN) is clamped instead of crashing.
    /// </summary>
    public static (int Hold, int Gap) Timing(double interval)
    {
        var milliseconds = double.IsNaN(interval) ? 240 : Math.Min(580, Math.Max(20, interval * 1000));
        var total = (int)Math.Round(milliseconds);
        // DECISION: keep the proven 80 ms hold when there is time; faster speeds hold for half the interval.
        var hold = Math.Min(80, total / 2);
        return (hold, total - hold);
    }

    // DECISION: remote apps can handle a line break after the keys that follow it (a new line
    // appeared a few characters late, and a blank line was lost), so give each one time to land.
    public const int LineBreakSettleMs = 300;

    public static int SettleAfter(string element) => TextRules.IsLineBreak(element) ? LineBreakSettleMs : 0;
}

public enum TypingTest { Short, Capitals, Symbols, SmartSymbols, Long }

public static class TypingTests
{
    public static string Title(this TypingTest test) => test switch
    {
        TypingTest.Short => "Short",
        TypingTest.Capitals => "Capitals",
        TypingTest.Symbols => "Symbols",
        TypingTest.SmartSymbols => "Smart symbols",
        _ => "Long",
    };

    public static string Text(this TypingTest test) => test switch
    {
        TypingTest.Short => "abc123",
        TypingTest.Capitals => "AbC123!",
        TypingTest.Symbols => "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~",
        TypingTest.SmartSymbols => "• Bullet – en — em “double” ‘single’ … 3×4÷2 → ≤ ≥ ≠ © ® ™",
        _ => "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs!\n"
            + "Invoice #2048: $1,299.50 (incl. 8% tax) due 2026-10-31; ref A-17/B.\n"
            + "\tIndented: test@example.com, C:\\Temp\\notes.txt, {braces} [brackets] <angles>.\n"
            + "Sphinx of black quartz, judge my vow? 0123456789 ~ ` ^ | _ + = \" '",
    };

    public static string Help(this TypingTest test) => test switch
    {
        TypingTest.Short => "Types abc123 at the slowest speed.",
        TypingTest.Capitals => "Types AbC123! at the slowest speed, to check Shift.",
        TypingTest.Symbols => "Types all 32 keyboard symbols at the slowest speed.",
        TypingTest.SmartSymbols => "Types bullets, smart quotes, and dashes as plain keys.",
        _ => "Types four lines with Tab and Return at your selected speed. Use a multi-line text field.",
    };

    // DECISION: short tests use the slowest speed to isolate the method; the long test checks the selected speed.
    public static bool UsesSelectedSpeed(this TypingTest test) => test == TypingTest.Long;
}

[Flags]
public enum Modifiers { None = 0, Alt = 1, Control = 2, Shift = 4, Windows = 8 }

/// <summary>A global shortcut: modifiers plus a Windows virtual-key code.</summary>
public sealed record Shortcut(Modifiers Modifiers, int Key)
{
    public const int Escape = 0x1B, F1 = 0x70, F24 = 0x87, Backslash = 0xDC, RightBracket = 0xDD;

    // DECISION: Control+Backslash by default. The hotkey consumes its key but not its modifiers,
    // which still reach the remote session: a lone Alt opens the Windows menu bar, and the
    // Windows key opens Start. Control alone is harmless there, and apps rarely use Ctrl+\.
    public static readonly Shortcut Standard = new(Modifiers.Control, Backslash);
    // DECISION: Control+] for Copy Text from Screen, next to the typing shortcut.
    public static readonly Shortcut Copy = new(Modifiers.Control, RightBracket);

    public bool IsFunctionKey => Key is >= F1 and <= F24;

    public string? Problem =>
        Key == Escape ? "Esc stops typing, so it cannot start it."
        : !IsFunctionKey && (Modifiers & (Modifiers.Control | Modifiers.Alt | Modifiers.Windows)) == 0
            ? "Add Control, Alt, or the Windows key, or use a function key such as F13."
        : null;

    public string? Caution =>
        (Modifiers & (Modifiers.Alt | Modifiers.Windows | Modifiers.Shift)) != 0
            ? "Alt, the Windows key, and Shift also reach the remote session and can trigger shortcuts there. Control alone is safest."
            : null;
}
