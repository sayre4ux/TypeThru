using System;
using System.Collections.Generic;
using System.Linq;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;

namespace TypeThru;

/// <summary>Small helpers so the windows read like the Mac app's SwiftUI views.</summary>
internal static class Ui
{
    public static TextBlock Text(string text, double size = 13, FontWeight? weight = null, bool secondary = false, bool wrap = true)
    {
        var block = new TextBlock { Text = text, FontSize = size, FontWeight = weight ?? FontWeights.Normal, TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap };
        if (secondary) block.Opacity = 0.7;
        return block;
    }

    public static Button Button(string label, Action action, bool accent = false, string? help = null)
    {
        var button = new Button { Content = label, Padding = new Thickness(12, 5, 12, 5), ToolTip = help };
        if (accent && Application.Current.TryFindResource("AccentButtonStyle") is Style style) button.Style = style;
        button.Click += (_, _) => action();
        return button;
    }

    public static StackPanel Stack(Orientation orientation, double spacing, params UIElement[] children)
    {
        var stack = new StackPanel { Orientation = orientation };
        foreach (var child in children)
        {
            if (child is FrameworkElement element && stack.Children.Count > 0)
                element.Margin = orientation == Orientation.Vertical ? new Thickness(0, spacing, 0, 0) : new Thickness(spacing, 0, 0, 0);
            stack.Children.Add(child);
        }
        return stack;
    }

    public static CheckBox Toggle(string label, bool value, Action<bool> changed)
    {
        var box = new CheckBox { Content = label, IsChecked = value };
        box.Checked += (_, _) => changed(true);
        box.Unchecked += (_, _) => changed(false);
        return box;
    }

    public static ComboBox Choice(IEnumerable<string> items, int selected, Action<int> changed)
    {
        var combo = new ComboBox { ItemsSource = items.ToList(), SelectedIndex = selected, MinWidth = 220 };
        combo.SelectionChanged += (_, _) => { if (combo.SelectedIndex >= 0) changed(combo.SelectedIndex); };
        return combo;
    }

    /// <summary>A row with a label on the left and a control on the right.</summary>
    public static Grid Row(string label, UIElement control)
    {
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var text = Text(label);
        text.VerticalAlignment = VerticalAlignment.Center;
        grid.Children.Add(text);
        Grid.SetColumn(control, 1);
        grid.Children.Add(control);
        return grid;
    }

    /// <summary>A rounded group, like a section of the Mac Settings form.</summary>
    public static Border Group(params UIElement[] children) => new()
    {
        Child = Stack(Orientation.Vertical, 10, children),
        Padding = new Thickness(14, 12, 14, 12),
        CornerRadius = new CornerRadius(8),
        Background = Application.Current.TryFindResource("CardBackgroundFillColorDefaultBrush") as Brush ?? new SolidColorBrush(Color.FromArgb(18, 128, 128, 128)),
        BorderBrush = Application.Current.TryFindResource("CardStrokeColorDefaultBrush") as Brush ?? new SolidColorBrush(Color.FromArgb(40, 128, 128, 128)),
        BorderThickness = new Thickness(1),
    };

    /// <summary>One line of release notes: bold, italic, and code; links show only their label.</summary>
    public static TextBlock Note(string line)
    {
        var bullet = line.StartsWith("- ", StringComparison.Ordinal);
        var block = new TextBlock { TextWrapping = TextWrapping.Wrap, FontSize = 13 };
        if (bullet) block.Inlines.Add(new Run("•  ") { Foreground = Brushes.CornflowerBlue });
        foreach (var part in InlineMarkdown.Parse(bullet ? line[2..] : line))
        {
            var run = new Run(part.Text);
            if (part.Bold) run.FontWeight = FontWeights.SemiBold;
            if (part.Italic) run.FontStyle = FontStyles.Italic;
            if (part.Code) run.FontFamily = new FontFamily("Cascadia Mono, Consolas");
            block.Inlines.Add(run);
        }
        return block;
    }
}

/// <summary>The panel from the notification-area icon, like the Mac menu bar panel.</summary>
internal sealed class PanelWindow : Window
{
    public PanelWindow(Controller app)
    {
        Title = "TypeThru";
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        ShowInTaskbar = false;
        Topmost = true;
        SizeToContent = SizeToContent.WidthAndHeight;
        Width = 360;
        var settings = app.Settings;

        var status = Ui.Stack(Orientation.Horizontal, 6,
            new System.Windows.Shapes.Ellipse { Width = 8, Height = 8, Fill = app.Ready ? Brushes.LimeGreen : Brushes.Orange, VerticalAlignment = VerticalAlignment.Center },
            Ui.Text(app.Status, 12, FontWeights.Medium, wrap: false));
        var header = new DockPanel();
        DockPanel.SetDock(status, Dock.Right);
        status.VerticalAlignment = VerticalAlignment.Center;
        header.Children.Add(status);
        header.Children.Add(Ui.Text("TypeThru", 18, FontWeights.SemiBold));

        var content = new List<UIElement> { header };
        if (app.Update is { } update)
        {
            var notes = Ui.Button("What's New", app.ShowUpdateNotes);
            content.Add(Ui.Group(
                Ui.Text("Update available", 13, FontWeights.SemiBold),
                Ui.Text(app.Installing ? app.UpdateStatus : $"Version {Updater.Display(update.Version)} · you have {Updater.Display(Updater.CurrentVersion)}", 12, secondary: true),
                Ui.Stack(Orientation.Horizontal, 8, notes, Ui.Button(app.Installing ? "Installing…" : "Install", app.InstallUpdate, accent: true))));
            notes.IsEnabled = !app.Installing;
        }
        var type = Ui.Button($"Type Clipboard    {HotKeys.Title(settings.Shortcut)}", () => { Close(); app.TypeClipboard(fromPanel: true); }, accent: true);
        type.HorizontalAlignment = HorizontalAlignment.Stretch;
        type.Padding = new Thickness(12, 9, 12, 9);
        type.FontSize = 15;
        type.IsEnabled = !app.Busy;
        content.Add(type);
        content.Add(Ui.Text("Esc, a mouse click, or switching windows stops typing.", 12, secondary: true));
        if (settings.CopyFromScreen)
        {
            var copy = Ui.Button($"Copy Text from Screen    {HotKeys.Title(Shortcut.Copy)}", () => { Close(); app.CopyFromScreen(); },
                help: "Drag over an area of the screen. Its text is read on this PC and copied.");
            copy.HorizontalAlignment = HorizontalAlignment.Stretch;
            copy.IsEnabled = !app.Busy;
            content.Add(copy);
        }
        content.Add(Ui.Row("Speed", Ui.Choice(Typing.Speeds.Select(s => s.Title), settings.SpeedIndex, i => { settings.SpeedIndex = i; app.Save(); })));
        content.Add(Ui.Text(settings.Speed.Detail, 12, secondary: true));
        content.Add(Ui.Toggle("Type line breaks as Shift+Enter", settings.ShiftReturn, v => { settings.ShiftReturn = v; app.Save(); }));
        content.Add(Ui.Text("Typing tests", 13, FontWeights.SemiBold));
        content.Add(Ui.Text("Starts in 3 seconds. Click the target text field first.", 12, secondary: true));
        var tests = new UniformGrid { Columns = 3 };
        foreach (var test in Enum.GetValues<TypingTest>())
        {
            var button = Ui.Button(test.Title(), () => { Close(); app.RunTest(test); }, help: test.Help());
            button.Margin = new Thickness(0, 0, 6, 6);
            button.IsEnabled = !app.Busy;
            tests.Children.Add(button);
        }
        content.Add(tests);
        var footer = new DockPanel();
        var quit = Ui.Button("Quit", app.Quit);
        DockPanel.SetDock(quit, Dock.Right);
        footer.Children.Add(quit);
        footer.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Children = { Ui.Button("Settings", () => { Close(); app.OpenSettings(); }) } });
        content.Add(footer);

        Content = new Border { Padding = new Thickness(18), Child = Ui.Stack(Orientation.Vertical, 12, content.ToArray()), Width = 360 };
        Deactivated += (_, _) => { if (IsVisible) Close(); };
        KeyDown += (_, e) => { if (e.Key == Key.Escape) Close(); };
        Loaded += (_, _) =>
        {
            // Above the notification area, at the bottom right of the main screen's work area.
            var area = SystemParameters.WorkArea;
            Left = area.Right - ActualWidth - 12;
            Top = area.Bottom - ActualHeight - 12;
        };
    }
}

/// <summary>Options people change rarely, like the Mac Settings window.</summary>
internal sealed class SettingsWindow : Window
{
    private readonly Controller app;
    private readonly TextBlock shortcutNote = Ui.Text("", 12);
    private readonly Button change;
    private readonly TextBlock shortcutLabel;
    private bool recording;

    public SettingsWindow(Controller app)
    {
        this.app = app;
        var settings = app.Settings;
        Title = "TypeThru Settings";
        Width = 520;
        MinWidth = 460;
        SizeToContent = SizeToContent.Height;
        MaxHeight = SystemParameters.WorkArea.Height - 40;
        ResizeMode = ResizeMode.CanMinimize;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        shortcutNote.Foreground = Brushes.DarkOrange;
        shortcutNote.Visibility = Visibility.Collapsed;
        shortcutLabel = Ui.Text(HotKeys.Title(settings.Shortcut), 13, FontWeights.Medium, wrap: false);
        shortcutLabel.VerticalAlignment = VerticalAlignment.Center;
        change = Ui.Button("Change", ToggleRecording);

        var launch = Ui.Toggle("Open TypeThru at login", Installation.LaunchAtLogin, v => Installation.LaunchAtLogin = v);
        var sections = new List<UIElement>
        {
            Ui.Text("General", 14, FontWeights.SemiBold),
            Ui.Group(launch, Ui.Row("Shortcut", Ui.Stack(Orientation.Horizontal, 10, shortcutLabel, change)), shortcutNote),
            Ui.Text("Typing", 14, FontWeights.SemiBold),
            Ui.Group(
                Ui.Row("Method", Ui.Choice(Enum.GetValues<TypingMethod>().Select(m => m.Title()), (int)settings.Method, i => { settings.Method = (TypingMethod)i; app.Save(); })),
                Ui.Text(settings.Method.Help(), 12, secondary: true),
                Ui.Row("Speed", Ui.Choice(Typing.Speeds.Select(s => s.Title), settings.SpeedIndex, i => { settings.SpeedIndex = i; app.Save(); })),
                Ui.Toggle("Replace symbols without a key (• → -)", settings.ReplaceSymbols, v => { settings.ReplaceSymbols = v; app.Save(); }),
                Ui.Row("Markdown (**bold**, *italic*)", Ui.Choice(Enum.GetValues<MarkdownMode>().Select(m => m.Title()), (int)settings.Markdown, i => { settings.Markdown = (MarkdownMode)i; app.Save(); })),
                Ui.Toggle("Type line breaks as Shift+Enter", settings.ShiftReturn, v => { settings.ShiftReturn = v; app.Save(); })),
            Ui.Text("Copy Text from Screen", 14, FontWeights.SemiBold),
            Ui.Group(
                Ui.Toggle($"Copy text from the screen with {HotKeys.Title(Shortcut.Copy)}", settings.CopyFromScreen, v => { settings.CopyFromScreen = v; app.Save(); app.RegisterHotKeys(); }),
                Ui.Text("Drag over an area. Its text is read on this PC by Windows' own text recognition and copied; the picture is never saved.", 12, secondary: true)),
            Ui.Text("Updates", 14, FontWeights.SemiBold),
            Ui.Group(
                Ui.Toggle("Check for updates automatically", settings.CheckUpdates, v => { settings.CheckUpdates = v; app.Save(); }),
                Ui.Row("Version", Ui.Text(Updater.Display(Updater.CurrentVersion), wrap: false)),
                UpdateButtons()),
            Ui.Text("Help", 14, FontWeights.SemiBold),
            Ui.Group(Ui.Stack(Orientation.Horizontal, 8, Ui.Button("What's New", app.ShowWhatsNew), Ui.Button("Diagnostics", app.ShowDiagnostics),
                Ui.Button("Uninstall TypeThru…", app.Uninstall))),
        };
        Content = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = new Border { Padding = new Thickness(20, 16, 20, 20), Child = Ui.Stack(Orientation.Vertical, 10, sections.ToArray()) },
        };
        PreviewKeyDown += Record;
        Closed += (_, _) => { if (recording) app.RegisterHotKeys(); };
    }

    private StackPanel UpdateButtons()
    {
        var buttons = Ui.Stack(Orientation.Horizontal, 8);
        if (app.Update is not null)
        {
            buttons.Children.Add(Ui.Button("What's New", app.ShowUpdateNotes));
            buttons.Children.Add(Ui.Button(app.Installing ? "Installing…" : "Install", app.InstallUpdate, accent: true));
        }
        buttons.Children.Add(Ui.Button("Check Now", () => app.CheckForUpdates(manual: true)));
        foreach (FrameworkElement child in buttons.Children) child.Margin = new Thickness(0, 0, 8, 0);
        return buttons;
    }

    private void Note(string? text)
    {
        shortcutNote.Text = text ?? "";
        shortcutNote.Visibility = text is null ? Visibility.Collapsed : Visibility.Visible;
    }

    /// <summary>The next key press in this window becomes the shortcut; the global one is off meanwhile.</summary>
    private void ToggleRecording()
    {
        recording = !recording;
        change.Content = recording ? "Cancel" : "Change";
        shortcutLabel.Text = recording ? "Press keys…" : HotKeys.Title(app.Settings.Shortcut);
        if (recording) { app.UnregisterHotKeys(); Note("Press the new shortcut. Esc cancels."); }
        else { app.RegisterHotKeys(); Note(null); }
    }

    private void Record(object sender, KeyEventArgs e)
    {
        if (!recording) return;
        var key = e.Key == Key.System ? e.SystemKey : e.Key;
        if (key is Key.LeftCtrl or Key.RightCtrl or Key.LeftAlt or Key.RightAlt or Key.LeftShift or Key.RightShift or Key.LWin or Key.RWin) return;
        e.Handled = true;
        var modifiers = Modifiers.None;
        if (Keyboard.Modifiers.HasFlag(ModifierKeys.Control)) modifiers |= Modifiers.Control;
        if (Keyboard.Modifiers.HasFlag(ModifierKeys.Alt)) modifiers |= Modifiers.Alt;
        if (Keyboard.Modifiers.HasFlag(ModifierKeys.Shift)) modifiers |= Modifiers.Shift;
        if (Keyboard.Modifiers.HasFlag(ModifierKeys.Windows)) modifiers |= Modifiers.Windows;
        if (key == Key.Escape && modifiers == Modifiers.None) { ToggleRecording(); return; }
        var shortcut = new Shortcut(modifiers, KeyInterop.VirtualKeyFromKey(key));
        if (shortcut.Problem is { } problem) { Note(problem); return; }
        if (shortcut == Shortcut.Copy && app.Settings.CopyFromScreen) { Note($"{HotKeys.Title(Shortcut.Copy)} copies text from the screen. Choose another shortcut."); return; }
        var previous = app.Settings.Shortcut;
        app.Settings.Shortcut = shortcut;
        recording = false;
        change.Content = "Change";
        if (app.RegisterHotKeys()) { app.Save(); Note(shortcut.Caution); }
        else
        {
            app.Settings.Shortcut = previous;
            app.RegisterHotKeys();
            Note($"Windows did not accept {HotKeys.Title(shortcut)}. Another app may use it.");
        }
        shortcutLabel.Text = HotKeys.Title(app.Settings.Shortcut);
    }
}

/// <summary>Release notes, after an update or before one.</summary>
internal sealed class WhatsNewWindow : Window
{
    public WhatsNewWindow(IReadOnlyList<ChangelogEntry> entries, string title)
    {
        Title = title;
        Width = 520;
        SizeToContent = SizeToContent.Height;
        MaxHeight = SystemParameters.WorkArea.Height * 0.75;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        var notes = new List<UIElement>();
        foreach (var entry in entries)
        {
            var section = new List<UIElement> { Ui.Text($"TypeThru {entry.Title}", 17, FontWeights.SemiBold) };
            section.AddRange(entry.Lines.Select(Ui.Note));
            notes.Add(Ui.Stack(Orientation.Vertical, 8, section.ToArray()));
        }
        var close = Ui.Button("Continue", Close, accent: true);
        close.IsDefault = true;
        close.HorizontalAlignment = HorizontalAlignment.Right;
        close.Margin = new Thickness(16);
        var layout = new DockPanel();
        DockPanel.SetDock(close, Dock.Bottom);
        layout.Children.Add(close);
        layout.Children.Add(new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = new Border { Padding = new Thickness(24, 20, 24, 8), Child = Ui.Stack(Orientation.Vertical, 22, notes.ToArray()) },
        });
        Content = layout;
    }
}

/// <summary>A short message above the notification area. It never takes focus and goes away by itself.</summary>
internal static class Toast
{
    private static Window? current;

    public static void Show(string text)
    {
        current?.Close();
        var window = new Window
        {
            WindowStyle = WindowStyle.None, ResizeMode = ResizeMode.NoResize, ShowInTaskbar = false, Topmost = true, ShowActivated = false,
            SizeToContent = SizeToContent.WidthAndHeight, AllowsTransparency = true, Background = Brushes.Transparent, IsHitTestVisible = false,
            Content = new Border
            {
                Child = Ui.Text(text, 13, FontWeights.Medium, wrap: false),
                Padding = new Thickness(16, 10, 16, 10),
                CornerRadius = new CornerRadius(18),
                Background = Application.Current.TryFindResource("SolidBackgroundFillColorBaseBrush") as Brush ?? Brushes.White,
                BorderBrush = new SolidColorBrush(Color.FromArgb(60, 128, 128, 128)),
                BorderThickness = new Thickness(1),
            },
        };
        window.Loaded += (_, _) =>
        {
            var area = SystemParameters.WorkArea;
            window.Left = area.Right - window.ActualWidth - 16;
            window.Top = area.Bottom - window.ActualHeight - 16;
        };
        current = window;
        window.Show();
        var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1.8) };
        timer.Tick += (_, _) => { timer.Stop(); window.Close(); if (current == window) current = null; };
        timer.Start();
    }
}
