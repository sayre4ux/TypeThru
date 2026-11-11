namespace TypeThru;

/// <summary>Release notes from CHANGELOG.md, built into the app, as on the Mac (Sources/WhatsNew.swift).</summary>
public static class Changelog
{
    /// <summary>Each "## " heading starts a version. Indented lines continue the line above.</summary>
    public static List<ChangelogEntry> Parse(string text)
    {
        var entries = new List<ChangelogEntry>();
        string? title = null;
        var lines = new List<string>();
        void Finish()
        {
            if (title is null) return;
            var version = UpdateFeed.Normalized(title);
            if (version.Length > 0) entries.Add(new ChangelogEntry(version, title, lines.ToList()));
        }
        foreach (var raw in text.Replace("\r\n", "\n").Split('\n'))
        {
            if (raw.StartsWith("## ", StringComparison.Ordinal))
            {
                Finish();
                title = raw[3..].Trim();
                lines.Clear();
            }
            else if (title is not null && raw.Trim().Length > 0)
            {
                if (raw.StartsWith(' ') && lines.Count > 0) lines[^1] += " " + raw.Trim();
                else lines.Add(raw);
            }
        }
        Finish();
        return entries;
    }

    /// <summary>Versions after <paramref name="last"/> up to <paramref name="current"/>, newest first.</summary>
    public static List<ChangelogEntry> Since(string last, string current, IEnumerable<ChangelogEntry> entries) =>
        entries.Where(e => UpdateFeed.IsNewer(e.Version, last) && !UpdateFeed.IsNewer(e.Version, current)).ToList();
}

public sealed record Inline(string Text, bool Bold = false, bool Italic = false, bool Code = false);

/// <summary>
/// Bold, italic, and code in one line of release notes. DECISION: no links; notes before an
/// update come from the internet. A [label](url) shows only its label.
/// </summary>
public static class InlineMarkdown
{
    public static List<Inline> Parse(string line)
    {
        var parts = new List<Inline>();
        var text = new System.Text.StringBuilder();
        bool bold = false, italic = false;
        void Flush(bool code = false)
        {
            if (text.Length > 0) parts.Add(new Inline(text.ToString(), bold && !code, italic && !code, code));
            text.Clear();
        }
        for (var i = 0; i < line.Length; i++)
        {
            var c = line[i];
            if (c == '`' && line.IndexOf('`', i + 1) is var end and > 0)
            {
                Flush();
                text.Append(line, i + 1, end - i - 1);
                Flush(code: true);
                i = end;
            }
            else if (c == '*' && i + 1 < line.Length && line[i + 1] == '*') { Flush(); bold = !bold; i++; }
            else if (c == '*') { Flush(); italic = !italic; }
            else if (c == '[' && line.IndexOf("](", i, StringComparison.Ordinal) is var mid and > 0
                     && line.IndexOf(')', mid) is var close and > 0)
            {
                text.Append(line, i + 1, mid - i - 1);
                i = close;
            }
            else text.Append(c);
        }
        Flush();
        return parts;
    }
}

/// <summary>A piece of recognized text and its box on screen, in pixels, origin at the top left.</summary>
public sealed record Piece(string Text, double X, double Y, double Width, double Height)
{
    public double MidY => Y + Height / 2;
    public double Bottom => Y + Height;
    public double Right => X + Width;
}

/// <summary>Copy Text from Screen: the same reading order as the Mac (ScreenText.layout).</summary>
public static class ScreenLayout
{
    /// <summary>
    /// Puts recognized pieces back in reading order: rows top to bottom, a blank line where a
    /// paragraph gap was, and a tab between pieces far apart on one row (table columns).
    /// </summary>
    public static string Layout(IEnumerable<Piece> pieces)
    {
        var rows = new List<List<Piece>>();
        foreach (var piece in pieces.Where(p => p.Text.Length > 0).OrderBy(p => p.MidY))
        {
            var last = rows.Count > 0 ? rows[^1][0] : null;
            if (last is not null && Math.Abs(last.MidY - piece.MidY) < Math.Min(last.Height, piece.Height) / 2) rows[^1].Add(piece);
            else rows.Add([piece]);
        }
        var text = new System.Text.StringBuilder();
        double? previousBottom = null;
        foreach (var row in rows)
        {
            var sorted = row.OrderBy(p => p.X).ToList();
            var height = sorted.Max(p => p.Height);
            if (previousBottom is double bottom)
            {
                text.Append('\n');
                if (sorted.Min(p => p.Y) - bottom > height * 0.9) text.Append('\n');
            }
            for (var i = 0; i < sorted.Count; i++)
            {
                if (i > 0) text.Append(sorted[i].X - sorted[i - 1].Right > height * 1.5 ? '\t' : ' ');
                text.Append(sorted[i].Text);
            }
            previousBottom = sorted.Max(p => p.Bottom);
        }
        return text.ToString();
    }
}
