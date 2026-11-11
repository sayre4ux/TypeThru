using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Windows.Interop;
using Microsoft.Win32;

namespace TypeThru;

/// <summary>Global shortcuts with RegisterHotKey, on a message-only window. No keyboard hook.</summary>
internal sealed class HotKeys : IDisposable
{
    private readonly HwndSource window;
    private readonly Dictionary<int, Action> actions = [];

    public HotKeys()
    {
        // HWND_MESSAGE: a window that is never shown and only receives messages.
        window = new HwndSource(new HwndSourceParameters("TypeThru hotkeys") { ParentWindow = new IntPtr(-3) });
        window.AddHook((IntPtr hwnd, int message, IntPtr wParam, IntPtr lParam, ref bool handled) =>
        {
            if (message == Native.WmHotKey && actions.TryGetValue(wParam.ToInt32(), out var action)) { handled = true; action(); }
            return IntPtr.Zero;
        });
    }

    /// <summary>Registers <paramref name="shortcut"/> under <paramref name="id"/>, replacing it. False if Windows refuses it.</summary>
    public bool Register(int id, Shortcut shortcut, Action action)
    {
        Unregister(id);
        if (!Native.RegisterHotKey(window.Handle, id, (uint)shortcut.Modifiers | Native.ModNoRepeat, (uint)shortcut.Key)) return false;
        actions[id] = action;
        return true;
    }

    public void Unregister(int id)
    {
        if (actions.Remove(id)) Native.UnregisterHotKey(window.Handle, id);
    }

    public void Dispose()
    {
        foreach (var id in new List<int>(actions.Keys)) Unregister(id);
        window.Dispose();
    }

    /// <summary>"Ctrl+\" with the key's label on the current layout.</summary>
    public static string Title(Shortcut shortcut)
    {
        var parts = new List<string>();
        if (shortcut.Modifiers.HasFlag(Modifiers.Control)) parts.Add("Ctrl");
        if (shortcut.Modifiers.HasFlag(Modifiers.Alt)) parts.Add("Alt");
        if (shortcut.Modifiers.HasFlag(Modifiers.Shift)) parts.Add("Shift");
        if (shortcut.Modifiers.HasFlag(Modifiers.Windows)) parts.Add("Win");
        string key;
        if (shortcut.IsFunctionKey) key = $"F{shortcut.Key - Shortcut.F1 + 1}";
        else
        {
            var character = Native.MapVirtualKeyExW((uint)shortcut.Key, Native.MapVkToChar, Native.GetKeyboardLayout(0)) & 0x7FFF;
            key = character > 32 ? ((char)character).ToString().ToUpperInvariant() : $"Key {shortcut.Key:X2}";
        }
        parts.Add(key);
        return string.Join("+", parts);
    }
}

/// <summary>Where TypeThru lives: installed per user, without an administrator password.</summary>
internal static class Installation
{
    public static readonly string Folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "TypeThru");
    public static readonly string InstalledExe = Path.Combine(Folder, "TypeThru.exe");
    public static readonly string DataFolder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "TypeThru");
    public static readonly string SettingsFile = Path.Combine(DataFolder, "settings.json");
    private static readonly string StartMenuShortcut = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "TypeThru.lnk");
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";

    public static string CurrentExe => Environment.ProcessPath ?? InstalledExe;

    public static bool IsInstalled => string.Equals(Path.GetFullPath(CurrentExe), Path.GetFullPath(InstalledExe), StringComparison.OrdinalIgnoreCase);

    /// <summary>Copies this exe to the per-user programs folder and adds it to the Start menu.</summary>
    public static void Install()
    {
        Directory.CreateDirectory(Folder);
        File.Copy(CurrentExe, InstalledExe, overwrite: true);
        CreateShortcut();
    }

    public static void CreateShortcut()
    {
        // The Windows Script Host shortcut object, so no extra library is needed.
        var shellType = Type.GetTypeFromProgID("WScript.Shell");
        if (shellType is null) return;
        dynamic shell = Activator.CreateInstance(shellType)!;
        var link = shell.CreateShortcut(StartMenuShortcut);
        link.TargetPath = InstalledExe;
        link.WorkingDirectory = Folder;
        link.Description = "Type the clipboard as key presses";
        link.Save();
    }

    public static bool LaunchAtLogin
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue("TypeThru") is string value && value.Contains(CurrentExe, StringComparison.OrdinalIgnoreCase);
        }
        set
        {
            using var key = Registry.CurrentUser.CreateSubKey(RunKey);
            if (value) key.SetValue("TypeThru", $"\"{CurrentExe}\"");
            else key.DeleteValue("TypeThru", throwOnMissingValue: false);
        }
    }

    /// <summary>Removes TypeThru's files, shortcut, settings, and login item. The exe is deleted after TypeThru quits.</summary>
    public static void Uninstall()
    {
        LaunchAtLogin = false;
        File.Delete(StartMenuShortcut);
        File.Delete(SettingsFile);
        if (Directory.Exists(DataFolder) && Directory.GetFileSystemEntries(DataFolder).Length == 0) Directory.Delete(DataFolder);
        if (!IsInstalled) return;
        // DECISION: a running exe cannot delete itself; cmd waits two seconds, then deletes only
        // TypeThru's own files and the folder if it is empty.
        var exe = InstalledExe;
        Process.Start(new ProcessStartInfo("cmd.exe", $"/c timeout /t 2 /nobreak >nul & del /f /q \"{exe}\" \"{exe}.old\" & rmdir \"{Folder}\"")
        {
            CreateNoWindow = true,
            UseShellExecute = false,
        });
    }
}
