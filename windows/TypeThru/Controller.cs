using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Threading;
using Forms = System.Windows.Forms;

namespace TypeThru;

/// <summary>The app: notification-area icon, shortcuts, typing, screen copy, updates. Like AppDelegate on the Mac.</summary>
internal sealed class Controller : IDisposable
{
    private const int TypeHotKey = 1, CopyHotKey = 2;
    private readonly HotKeys hotKeys = new();
    private readonly Forms.NotifyIcon tray = new();
    private readonly DispatcherTimer updateTimer = new() { Interval = TimeSpan.FromHours(1) };
    private PanelWindow? panel;
    private SettingsWindow? settingsWindow;
    private string lastAttempt = "No attempt in this session.";
    private string lastCopy = "No text copied from the screen in this session.";
    private bool copying;

    public AppSettings Settings { get; }
    public bool Busy { get; private set; }
    public Release? Update { get; private set; }
    public bool Installing { get; private set; }
    public string UpdateStatus { get; private set; } = "";
    // Windows needs no helper or permission to send keys, so TypeThru is always ready.
    public bool Ready => true;
    public string Status => Busy ? "Typing…" : "Ready";

    public Controller()
    {
        Settings = AppSettings.Parse(File.Exists(Installation.SettingsFile) ? SafeRead(Installation.SettingsFile) : null);
    }

    private static string? SafeRead(string path)
    {
        try { return File.ReadAllText(path); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return null; }
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(Installation.DataFolder);
            var temporary = Installation.SettingsFile + ".tmp";
            File.WriteAllText(temporary, Settings.Serialize());
            File.Move(temporary, Installation.SettingsFile, overwrite: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
    }

    public void Start()
    {
        Updater.CleanUp();
        tray.Icon = System.Drawing.Icon.ExtractAssociatedIcon(Installation.CurrentExe);
        tray.Text = "TypeThru";
        tray.Visible = true;
        tray.MouseClick += (_, e) => { if (e.Button == Forms.MouseButtons.Left) TogglePanel(); };
        var menu = new Forms.ContextMenuStrip();
        menu.Items.Add("Type Clipboard", null, (_, _) => TypeClipboard(fromPanel: true));
        menu.Items.Add("Copy Text from Screen", null, (_, _) => CopyFromScreen());
        menu.Items.Add("Settings", null, (_, _) => OpenSettings());
        menu.Items.Add("What's New", null, (_, _) => ShowWhatsNew());
        menu.Items.Add(new Forms.ToolStripSeparator());
        menu.Items.Add("Quit TypeThru", null, (_, _) => Quit());
        tray.ContextMenuStrip = menu;

        if (!RegisterHotKeys())
            Alert($"Could not use {HotKeys.Title(Settings.Shortcut)}", "Another app may use this shortcut. Choose a different one in TypeThru Settings.");
        if (Onboarding()) return;
        updateTimer.Tick += (_, _) => CheckForUpdates(manual: false);
        updateTimer.Start();
        _ = Task.Delay(TimeSpan.FromSeconds(10)).ContinueWith(_ => Application.Current.Dispatcher.Invoke(() => CheckForUpdates(manual: false)));
        ShowWhatsNewAfterUpdate();
    }

    /// <summary>First run: install for this user, then the login question. Each is asked once. True when quitting.</summary>
    private bool Onboarding()
    {
        if (!Installation.IsInstalled && !Settings.InstallAsked)
        {
            Settings.InstallAsked = true;
            Save();
            if (Ask("Install TypeThru?", $"TypeThru copies itself to {Installation.Folder} and adds itself to the Start menu. No administrator password is needed, and updates install from the app.", "Install", "Not Now"))
            {
                try
                {
                    Installation.Install();
                    Process.Start(new ProcessStartInfo(Installation.InstalledExe) { UseShellExecute = true });
                    Quit();
                    return true;
                }
                catch (Exception e) when (e is IOException or UnauthorizedAccessException)
                {
                    Alert("Could not install TypeThru", e.Message);
                }
            }
        }
        if (!Settings.LaunchAtLoginAsked)
        {
            Settings.LaunchAtLoginAsked = true;
            Save();
            if (!Installation.LaunchAtLogin && Ask("Open TypeThru at login?", $"TypeThru starts when you sign in, so {HotKeys.Title(Settings.Shortcut)} always works. You can change this in Settings.", "Open at Login", "Not Now"))
                Installation.LaunchAtLogin = true;
            Toast.Show($"TypeThru is in the notification area. Press {HotKeys.Title(Settings.Shortcut)} to type the clipboard.");
        }
        return false;
    }

    public bool RegisterHotKeys()
    {
        var ok = hotKeys.Register(TypeHotKey, Settings.Shortcut, () => TypeClipboard(fromPanel: false));
        if (Settings.CopyFromScreen) hotKeys.Register(CopyHotKey, Shortcut.Copy, CopyFromScreen);
        else hotKeys.Unregister(CopyHotKey);
        return ok;
    }

    public void UnregisterHotKeys() => hotKeys.Unregister(TypeHotKey);

    private void TogglePanel()
    {
        if (panel is { IsVisible: true }) { panel.Close(); return; }
        panel = new PanelWindow(this);
        panel.Closed += (_, _) => panel = null;
        panel.Show();
        panel.Activate();
    }

    public void OpenSettings()
    {
        if (settingsWindow is null)
        {
            settingsWindow = new SettingsWindow(this);
            settingsWindow.Closed += (_, _) => settingsWindow = null;
        }
        settingsWindow.Show();
        settingsWindow.Activate();
    }

    public void RunTest(TypingTest test) => Start(fromPanel: true, test);

    public void TypeClipboard(bool fromPanel) => Start(fromPanel, null);

    /// <summary>
    /// From the shortcut, types into the window in front. From the panel or menu, first gives the
    /// user a moment to click the target field (three seconds for tests), as on the Mac.
    /// </summary>
    private async void Start(bool fromPanel, TypingTest? test)
    {
        if (Busy) { System.Media.SystemSounds.Beep.Play(); return; }
        if (fromPanel)
        {
            SetBusy(true);
            await Task.Delay(test is null ? 1000 : 3000);
            SetBusy(false);
        }
        Trigger(Native.GetForegroundWindow(), fromPanel, test);
    }

    private static string? ClipboardText()
    {
        for (var attempt = 0; attempt < 5; attempt++)
        {
            try { return Clipboard.ContainsText() ? Clipboard.GetText() : null; }
            catch (System.Runtime.InteropServices.COMException) { Thread.Sleep(50); }
        }
        return null;
    }

    private void Trigger(IntPtr window, bool fromPanel, TypingTest? test)
    {
        var method = Settings.Method;
        var time = DateTime.Now.ToLongTimeString();
        lastAttempt = $"Attempt: {time}\nTrigger: {(test is { } t ? $"{t.Title()} test" : fromPanel ? "panel" : "shortcut")}\nMethod: {method.Title()}";
        var copied = test?.Text() ?? ClipboardText() ?? "";
        var original = TextRules.WithoutTrailingLineBreaks(copied);
        if (original.Length == 0)
        {
            lastAttempt += "\nNot started: clipboard has no text.";
            Alert("Nothing typed", "The clipboard has no text. You can try a typing test from the TypeThru panel.");
            return;
        }
        if (original.Length < copied.Length) lastAttempt += "\nLeft out the line break at the end.";
        var map = new KeyMap(window, Settings.ShiftReturn);
        if (Settings.ShiftReturn) lastAttempt += "\nLine breaks: Shift+Enter.";
        // The smart symbols test exists to show the replacements, so it always replaces.
        var (text, replaced) = (Settings.ReplaceSymbols || test == TypingTest.SmartSymbols) && method != TypingMethod.Unicode
            ? TextRules.ReplaceUntypable(original, e => map.Typeable(e, method)) : (original, 0);
        if (replaced > 0) lastAttempt += $"\nReplaced {replaced} symbols that have no key.";
        var formats = new Dictionary<int, List<TextFormat>>();
        if (Settings.Markdown != MarkdownMode.Keep && test is null)
        {
            var parsed = TextRules.Markdown(text);
            if (parsed.Spans > 0)
            {
                text = parsed.Text;
                if (Settings.Markdown == MarkdownMode.Format && map["b"] is not null && map["i"] is not null) formats = parsed.Formats;
                lastAttempt += $"\nMarkdown: {parsed.Spans} marked spans, " + (formats.Count == 0 ? "marks removed." : "typed as bold or italic.");
            }
        }
        if (TextRules.Unsupported(text, e => map.Typeable(e, method)).Count > 0)
        {
            lastAttempt += "\nNot started: unsupported characters.";
            Alert("Nothing typed", method == TypingMethod.Unicode
                ? "The text contains a character too long for a Unicode key event."
                : "Some characters have no key on the current keyboard layout. Try Unicode text in Settings, or check your keyboard layout.");
            return;
        }
        Native.GetWindowThreadProcessId(window, out var process);
        if (window == IntPtr.Zero || process == Environment.ProcessId)
        {
            lastAttempt += "\nNot started: no target window.";
            Alert("Nothing typed", "Click the text field you want to type into, then try again.");
            return;
        }
        if (Native.IsElevated(process) && !Native.IsElevated((uint)Environment.ProcessId))
        {
            lastAttempt += "\nNot started: the target runs as administrator.";
            Alert("Nothing typed", "Windows does not let TypeThru type into an app running as administrator. Open that app normally, or run TypeThru as administrator.");
            return;
        }

        SetBusy(true);
        var interval = test is { } chosen && !chosen.UsesSelectedSpeed() ? Typing.Speeds[0].Interval : Settings.Speed.Interval;
        var hotKey = Settings.Shortcut;
        var elements = TextRules.Elements(text);
        lastAttempt += $"\nSpeed: {(int)(interval * 1000)} ms per character\nWaiting to start.";
        Task.Run(() =>
        {
            TypeResult result;
            var released = WaitForRelease(hotKey, includeKey: !fromPanel);
            if (released) Thread.Sleep(fromPanel ? 150 : 750);
            if (!released) result = new TypeResult.Cancelled(0, elements.Count, "the shortcut or modifier keys were not released");
            else if (Native.GetForegroundWindow() != window) result = new TypeResult.Cancelled(0, elements.Count, "the target window changed before typing");
            else result = new Typer(map, method, interval, window).Type(elements, formats);
            Application.Current.Dispatcher.Invoke(() =>
            {
                SetBusy(false);
                if (result is TypeResult.Cancelled stopped)
                {
                    lastAttempt += $"\nStopped after sending {stopped.Typed} of {stopped.Total} characters: {stopped.Reason}.";
                    System.Media.SystemSounds.Beep.Play();
                    Alert("Typing stopped", $"Stopped after sending {stopped.Typed} of {stopped.Total} characters because {stopped.Reason}.");
                }
                else lastAttempt += $"\nSent key presses for {elements.Count} characters. Remote acceptance is unverified.";
            });
        });
    }

    /// <summary>Waits until the shortcut's modifiers and key are up, so they do not mix with the typed keys.</summary>
    private static bool WaitForRelease(Shortcut shortcut, bool includeKey)
    {
        int[] keys = [Native.VkControl, Native.VkMenu, Native.VkShift, Native.VkLWin, Native.VkRWin];
        for (var waited = 0; waited < 5000; waited += 10)
        {
            if (!keys.Any(Native.IsDown) && !(includeKey && Native.IsDown(shortcut.Key))) return true;
            Thread.Sleep(10);
        }
        return false;
    }

    private void SetBusy(bool value)
    {
        Busy = value;
        tray.Text = value ? "TypeThru: typing… Esc stops" : "TypeThru";
    }

    public async void CopyFromScreen()
    {
        if (!Settings.CopyFromScreen || Busy || copying) { System.Media.SystemSounds.Beep.Play(); return; }
        copying = true;
        var time = DateTime.Now.ToLongTimeString();
        try
        {
            switch (await ScreenCopy.Copy())
            {
                case CopyOutcome.Copied copied:
                    lastCopy = $"Last copy from the screen: {time}, {copied.Lines} lines, {copied.Characters} characters.";
                    Toast.Show(copied.Lines == 1 ? "Copied 1 line of text" : $"Copied {copied.Lines} lines of text");
                    break;
                case CopyOutcome.NoText:
                    lastCopy = $"Last copy from the screen: {time}, no text found.";
                    Toast.Show("No text found in the selection");
                    break;
                case CopyOutcome.Failed failed:
                    lastCopy = $"Last copy from the screen: {time}, failed.";
                    Alert("Could not copy text", failed.Reason);
                    break;
            }
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            lastCopy = $"Last copy from the screen: {time}, failed.";
            Alert("Could not copy text", e.Message);
        }
        finally { copying = false; }
    }

    public async void CheckForUpdates(bool manual)
    {
        if (!manual && (!Settings.CheckUpdates || Settings.LastUpdateCheck is { } last && DateTimeOffset.Now - last < TimeSpan.FromHours(23))) return;
        UpdateStatus = "Checking…";
        try
        {
            Update = await Updater.Check();
            Settings.LastUpdateCheck = DateTimeOffset.Now;
            Save();
            UpdateStatus = Update is { } release ? $"Version {Updater.Display(release.Version)} is available." : "TypeThru is up to date.";
            if (manual && Update is null) Alert("TypeThru is up to date", $"You have version {Updater.Display(Updater.CurrentVersion)}.");
            if (Update is not null && !manual) tray.ShowBalloonTip(5000, "TypeThru update available", UpdateStatus + " Open the TypeThru panel to install it.", Forms.ToolTipIcon.Info);
            if (manual && Update is not null) TogglePanelOpen();
        }
        catch (Exception e) when (e is System.Net.Http.HttpRequestException or TaskCanceledException or IOException)
        {
            UpdateStatus = "Could not check for updates.";
            if (manual) Alert("Could not check for updates", "Check your internet connection and try again.");
        }
    }

    private void TogglePanelOpen()
    {
        if (panel is null) TogglePanel();
    }

    public async void InstallUpdate()
    {
        if (Update is not { } release || Installing) return;
        if (release.Download is null)
        {
            Process.Start(new ProcessStartInfo(release.Page.AbsoluteUri) { UseShellExecute = true });
            return;
        }
        Installing = true;
        UpdateStatus = "Downloading and checking…";
        panel?.Close();
        try
        {
            await Updater.Install(release);
            Process.Start(new ProcessStartInfo(Installation.CurrentExe) { UseShellExecute = false, Arguments = "--updated" });
            Quit();
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            Installing = false;
            UpdateStatus = "The update was not installed.";
            if (Ask("The update was not installed", $"{e.Message}\n\nYou can download it from the release page instead.", "Open Release Page", "Close"))
                Process.Start(new ProcessStartInfo(release.Page.AbsoluteUri) { UseShellExecute = true });
        }
    }

    private static List<ChangelogEntry> Bundled()
    {
        using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("CHANGELOG.md");
        return stream is null ? [] : Changelog.Parse(new StreamReader(stream).ReadToEnd());
    }

    /// <summary>After an update, the notes of every version since the one last run here. Nothing for a new user.</summary>
    private void ShowWhatsNewAfterUpdate()
    {
        var current = Updater.CurrentVersion;
        var last = Settings.LastRunVersion;
        Settings.LastRunVersion = current;
        Save();
        if (last is null || !UpdateFeed.IsNewer(current, last)) return;
        var entries = Changelog.Since(last, current, Bundled());
        if (entries.Count > 0) new WhatsNewWindow(entries, entries.Count == 1 ? "What's New in TypeThru" : "What's New Since Your Last Version").Show();
    }

    public void ShowWhatsNew()
    {
        var entries = Bundled().Where(e => !UpdateFeed.IsNewer(e.Version, Updater.CurrentVersion)).Take(3).ToList();
        new WhatsNewWindow(entries, "What's New in TypeThru").Show();
    }

    public void ShowUpdateNotes()
    {
        if (Update is not { } release) return;
        panel?.Close();
        // A release remembered from an earlier check has no notes yet; GitHub's page has them.
        if (release.Notes.Count == 0) Process.Start(new ProcessStartInfo(release.Page.AbsoluteUri) { UseShellExecute = true });
        else new WhatsNewWindow(release.Notes, $"What's New in TypeThru {Updater.Display(release.Version)}").Show();
    }

    public void ShowDiagnostics() =>
        Alert("TypeThru Diagnostics",
            $"Version: {Updater.Display(Updater.CurrentVersion)}\nInstalled: {(Installation.IsInstalled ? "yes" : $"no, running from {Path.GetDirectoryName(Installation.CurrentExe)}")}\n" +
            $"Shortcut: {HotKeys.Title(Settings.Shortcut)}\nCopy Text from Screen: {(Settings.CopyFromScreen ? HotKeys.Title(Shortcut.Copy) : "off")}\n" +
            $"Running as administrator: {(Native.IsElevated((uint)Environment.ProcessId) ? "yes" : "no")}\n{lastCopy}\n\n{lastAttempt}\n\n" +
            "Sent key presses do not confirm that the remote session accepted them. Clipboard contents are not recorded.");

    public void Uninstall()
    {
        if (!Ask("Uninstall TypeThru?", "TypeThru removes its Start menu shortcut, login item, settings, and program files, then quits.", "Uninstall", "Cancel")) return;
        Installation.Uninstall();
        Quit();
    }

    public void Quit()
    {
        Dispose();
        Application.Current.Shutdown();
    }

    private bool disposed;

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        tray.Visible = false;
        tray.Dispose();
        hotKeys.Dispose();
        updateTimer.Stop();
    }

    public static void Alert(string title, string message) =>
        MessageBox.Show(message, title, MessageBoxButton.OK, MessageBoxImage.Information);

    private static bool Ask(string title, string message, string yes, string no) =>
        MessageBox.Show($"{message}\n\nYes: {yes}\nNo: {no}", title, MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes;
}
