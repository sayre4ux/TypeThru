using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;

namespace TypeThru;

public readonly record struct KeyStroke(int Vk, bool Shift);

/// <summary>The key for each character on the keyboard layout of the app being typed into.</summary>
internal sealed class KeyMap
{
    private readonly IntPtr layout;
    private readonly bool shiftReturn;

    public KeyMap(IntPtr window, bool shiftReturn)
    {
        layout = Native.GetKeyboardLayout(Native.GetWindowThreadProcessId(window, out _));
        this.shiftReturn = shiftReturn;
    }

    public IntPtr Layout => layout;

    /// <summary>The key and Shift for a character, or null when it needs another modifier or has no key.</summary>
    public KeyStroke? this[string element]
    {
        get
        {
            if (TextRules.IsLineBreak(element)) return new KeyStroke(Native.VkReturn, shiftReturn);
            if (element == "\t") return new KeyStroke(Native.VkTab, false);
            if (element.Length != 1 || char.IsControl(element[0])) return null;
            var result = Native.VkKeyScanExW(element[0], layout);
            if (result == -1) return null;
            var state = (result >> 8) & 0xFF;
            // DECISION: Shift only, as on the Mac. AltGr (Control+Alt) would also reach the remote session.
            return state is 0 or 1 ? new KeyStroke(result & 0xFF, state == 1) : null;
        }
    }

    public bool Typeable(string element, TypingMethod method) =>
        method == TypingMethod.Unicode ? element.Length <= 20 && (element == "\t" || TextRules.IsLineBreak(element) || !char.IsControl(element[0]))
        : this[element] is not null;
}

public abstract record TypeResult
{
    public sealed record Done : TypeResult;
    public sealed record Cancelled(int Typed, int Total, string Reason) : TypeResult;
}

/// <summary>Sends key presses with SendInput. Construction and sending stay together so a modifier is never left held.</summary>
internal sealed class Typer(KeyMap map, TypingMethod method, double interval, IntPtr target)
{
    private const ushort ScanShift = 0x2A, ScanControl = 0x1D;
    private readonly (int Hold, int Gap) timing = Typing.Timing(interval);

    public TypeResult Type(List<string> elements, Dictionary<int, List<TextFormat>> formats)
    {
        var total = elements.Count;
        using var watcher = new StopWatcher(target);
        bool Toggle(int index)
        {
            if (!formats.TryGetValue(index, out var list)) return true;
            foreach (var format in list)
            {
                if (map[format.Key().ToString()] is not { } key || !Press(key with { Shift = false }, control: true)) return false;
            }
            return true;
        }
        for (var index = 0; index < total; index++)
        {
            if (watcher.StopReason is { } reason) return new TypeResult.Cancelled(index, total, reason);
            if (!Toggle(index)) return new TypeResult.Cancelled(index, total, "bold or italic could not be switched");
            var element = elements[index];
            var sent = method == TypingMethod.Unicode && element != "\t" && !TextRules.IsLineBreak(element)
                ? SendUnicode(element)
                : map[element] is { } stroke && Press(stroke, control: false);
            if (!sent) return new TypeResult.Cancelled(index, total, "Windows did not accept the key presses. A window running as administrator or the lock screen may be in front");
            Thread.Sleep(Typing.SettleAfter(element));
        }
        return Toggle(total) ? new TypeResult.Done() : new TypeResult.Cancelled(total, total, "bold or italic could not be switched off");
    }

    private bool Press(KeyStroke stroke, bool control)
    {
        var modifier = control ? (Native.VkControl, ScanControl) : stroke.Shift ? (Native.VkShift, ScanShift) : ((int, ushort)?)null;
        if (modifier is { } held)
        {
            if (!Send(Key(held.Item1, held.Item2, down: true))) return false;
            Thread.Sleep(20);
        }
        var scan = Native.MapVirtualKeyExW((uint)stroke.Vk, Native.MapVkToVscEx, map.Layout);
        var ok = Send(Key(stroke.Vk, (ushort)scan, down: true));
        if (ok) Thread.Sleep(timing.Hold);
        // Release in any case, so a key or modifier is never left held.
        ok &= Send(Key(stroke.Vk, (ushort)scan, down: false));
        if (modifier is { } release) ok &= Send(Key(release.Item1, release.Item2, down: false));
        Thread.Sleep(timing.Gap);
        return ok;
    }

    private Native.Input Key(int vk, ushort scan, bool down)
    {
        var flags = down ? 0u : Native.KeyUp;
        var input = new Native.KeyboardInput { Vk = (ushort)vk, Scan = (ushort)(scan & 0xFF) };
        if (method == TypingMethod.ScanCodes)
        {
            // Remote desktop and VDI clients read hardware scan codes, not key codes.
            input.Vk = 0;
            flags |= Native.ScanCode;
            if ((scan & 0xFF00) == 0xE000) flags |= Native.ExtendedKey;
        }
        input.Flags = flags;
        return new Native.Input { Type = Native.InputKeyboard, Data = new Native.InputUnion { Keyboard = input } };
    }

    private bool SendUnicode(string element)
    {
        foreach (var unit in element)
        {
            var down = new Native.KeyboardInput { Scan = unit, Flags = Native.Unicode };
            var up = down with { Flags = Native.Unicode | Native.KeyUp };
            if (!Send(new Native.Input { Type = Native.InputKeyboard, Data = new Native.InputUnion { Keyboard = down } }, new Native.Input { Type = Native.InputKeyboard, Data = new Native.InputUnion { Keyboard = up } }))
                return false;
        }
        Thread.Sleep(timing.Hold + timing.Gap);
        return true;
    }

    private static bool Send(params Native.Input[] inputs) =>
        Native.SendInput((uint)inputs.Length, inputs, Marshal.SizeOf<Native.Input>()) == inputs.Length;
}

/// <summary>Stops typing on Esc, a new mouse click, or a change of the front window. Polls every 10 ms.</summary>
internal sealed class StopWatcher : IDisposable
{
    private readonly object gate = new();
    private string? reason;
    private volatile bool finished;

    public StopWatcher(IntPtr target)
    {
        int[] buttons = [Native.VkLButton, Native.VkRButton, Native.VkMButton];
        // Buttons already down when typing starts (finishing a click on the target field) do not count.
        var wasDown = Array.ConvertAll(buttons, Native.IsDown);
        new Thread(() =>
        {
            while (!finished)
            {
                string? found = null;
                if (Native.IsDown(Native.VkEscape)) found = "Esc pressed";
                var down = Array.ConvertAll(buttons, Native.IsDown);
                for (var i = 0; i < buttons.Length; i++) if (down[i] && !wasDown[i]) found = "the mouse was clicked";
                wasDown = down;
                if (Native.GetForegroundWindow() != target) found = "the front window changed";
                if (found is not null) { lock (gate) reason ??= found; return; }
                Thread.Sleep(10);
            }
        }) { IsBackground = true, Name = "TypeThru stop watcher" }.Start();
    }

    public string? StopReason { get { lock (gate) return reason; } }

    public void Dispose() => finished = true;
}
