using System.Globalization;

namespace TypeThru;

/// <summary>Formatting typed as Control plus this letter, as Word, Outlook, and most editors use.</summary>
public enum TextFormat { Bold, Italic }

/// <summary>What happens to Markdown marks such as **bold** and *italic* in the copied text.</summary>
public enum MarkdownMode { Keep, RemoveMarks, Format }

/// <summary>
/// The text rules shared with the Mac app (Sources/main.swift, Typer). Text is handled as
/// user-perceived characters (text elements), so "\r\n", "é", and emoji count as one, as in Swift.
/// </summary>
public static class TextRules
{
    public static string Title(this MarkdownMode mode) => mode switch
    {
        MarkdownMode.Keep => "Type as is",
        MarkdownMode.RemoveMarks => "Remove the marks",
        _ => "Bold and italic",
    };

    public static char Key(this TextFormat format) => format == TextFormat.Bold ? 'b' : 'i';

    /// <summary>The text as a list of user-perceived characters.</summary>
    public static List<string> Elements(string text)
    {
        var result = new List<string>();
        var e = StringInfo.GetTextElementEnumerator(text);
        while (e.MoveNext()) result.Add((string)e.Current);
        return result;
    }

    public static bool IsLineBreak(string element) => element is "\n" or "\r" or "\r\n";

    // Symbols with no key on common layouts, typed as plain keys that read the same.
    // DECISION: letters (such as é) are not replaced, because dropping an accent changes a word.
    public static readonly IReadOnlyDictionary<string, string> Replacements = new Dictionary<string, string>
    {
        ["•"] = "-", ["◦"] = "-", ["▪"] = "-", ["▫"] = "-", ["‣"] = "-", ["⁃"] = "-", ["●"] = "-", ["○"] = "-", ["■"] = "-", ["□"] = "-",
        ["·"] = "-", ["∙"] = "-", ["‐"] = "-", ["‑"] = "-", ["‒"] = "-", ["–"] = "-", ["−"] = "-", ["—"] = "--", ["―"] = "--",
        ["‘"] = "'", ["’"] = "'", ["‚"] = "'", ["′"] = "'", ["“"] = "\"", ["”"] = "\"", ["„"] = "\"", ["″"] = "\"", ["«"] = "\"", ["»"] = "\"",
        ["…"] = "...", ["×"] = "x", ["÷"] = "/", ["→"] = "->", ["←"] = "<-", ["⇒"] = "=>", ["≤"] = "<=", ["≥"] = ">=", ["≠"] = "!=",
        ["©"] = "(c)", ["®"] = "(R)", ["™"] = "(TM)",
        ["\u00A0"] = " ", ["\u2002"] = " ", ["\u2003"] = " ", ["\u2009"] = " ", ["\u202F"] = " ",
        ["\u200B"] = "", ["\uFEFF"] = "",
        // Line and paragraph separators, and Word's manual line break, become a normal line break.
        ["\u2028"] = "\n", ["\u2029"] = "\n", ["\u000B"] = "\n", ["\u0085"] = "\n",
    };

    /// <summary>Replaces symbols that have no key, when every replacement character has one.</summary>
    public static (string Text, int Replaced) ReplaceUntypable(string text, Func<string, bool> typeable)
    {
        var result = new System.Text.StringBuilder();
        var replaced = 0;
        foreach (var element in Elements(text))
        {
            if (!typeable(element) && Replacements.TryGetValue(element, out var substitute)
                && Elements(substitute).All(typeable))
            {
                result.Append(substitute);
                replaced++;
            }
            else
            {
                result.Append(element);
            }
        }
        return (result.ToString(), replaced);
    }

    /// <summary>The characters that cannot be typed, in order, without duplicates.</summary>
    public static List<string> Unsupported(string text, Func<string, bool> typeable)
    {
        var seen = new HashSet<string>();
        return Elements(text).Where(e => !typeable(e) && seen.Add(e)).ToList();
    }

    /// <summary>Leaves out line breaks at the end, so a copied command does not run by itself in a remote terminal.</summary>
    public static string WithoutTrailingLineBreaks(string text)
    {
        var elements = Elements(text);
        while (elements.Count > 0 && IsNewline(elements[^1])) elements.RemoveAt(elements.Count - 1);
        return string.Concat(elements);
    }

    private static bool IsNewline(string element) =>
        IsLineBreak(element) || element is "\u2028" or "\u2029" or "\u000B" or "\u000C" or "\u0085";

    private static bool InWord(string element) => element.Length > 0 && char.IsLetterOrDigit(element, 0);

    /// <summary>
    /// Removes Markdown bold and italic marks and returns, by character position in the result,
    /// where formatting switches on or off. Marks count only around text on one line and not
    /// inside a word, so 2**8, 5 * 3, *.txt, and **kwargs stay as they are.
    /// </summary>
    public static (string Text, Dictionary<int, List<TextFormat>> Formats, int Spans) Markdown(string text)
    {
        var chars = Elements(text);
        var n = chars.Count;
        var removed = new bool[n];
        var styles = new HashSet<TextFormat>[n];
        for (var i = 0; i < n; i++) styles[i] = [];
        var spans = 0;

        bool Word(int i) => i >= 0 && i < n && InWord(chars[i]);
        bool Mark(string d, int i) => i >= 0 && i + d.Length <= n
            && Enumerable.Range(0, d.Length).All(k => chars[i + k] == d[k].ToString() && !removed[i + k]);
        bool Space(int i) => string.IsNullOrWhiteSpace(chars[i]);

        // DECISION: asterisks only; underscores are common in code and file names (__init__).
        // Longest marks first, so ***both*** and **bold** are not read as italics.
        (string, TextFormat[])[] marks = [("***", [TextFormat.Bold, TextFormat.Italic]), ("**", [TextFormat.Bold]), ("*", [TextFormat.Italic])];
        foreach (var (delimiter, format) in marks)
        {
            var m = delimiter.Length;
            bool RunsOn(int i) => i >= 0 && i < n && chars[i] == "*" && !removed[i];
            var i = 0;
            while (i + m < n)
            {
                if (!Mark(delimiter, i) || Word(i - 1) || RunsOn(i - 1) || RunsOn(i + m) || Space(i + m)) { i++; continue; }
                int? close = null;
                for (var j = i + m + 1; j + m <= n && !IsNewline(chars[j - 1]); j++)
                {
                    if (Mark(delimiter, j) && !Space(j - 1) && !RunsOn(j - 1) && !RunsOn(j + m) && !Word(j + m)) { close = j; break; }
                }
                if (close is not int c) { i++; continue; }
                for (var k = 0; k < m; k++) { removed[i + k] = true; removed[c + k] = true; }
                for (var k = i + m; k < c; k++) if (!removed[k]) styles[k].UnionWith(format);
                spans++;
                i = c + m;
            }
        }

        var result = new List<string>();
        var formats = new Dictionary<int, List<TextFormat>>();
        var current = new HashSet<TextFormat>();
        for (var k = 0; k < n; k++)
        {
            if (removed[k]) continue;
            if (!styles[k].SetEquals(current))
            {
                var change = new HashSet<TextFormat>(current);
                change.SymmetricExceptWith(styles[k]);
                formats[result.Count] = change.Order().ToList();
                current = new HashSet<TextFormat>(styles[k]);
            }
            result.Add(chars[k]);
        }
        if (current.Count > 0) formats[result.Count] = current.Order().ToList();
        return (string.Concat(result), formats, spans);
    }
}
