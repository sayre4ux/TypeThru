using System;
using System.Threading;
using System.Windows;

namespace TypeThru;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        // One TypeThru per Windows user: a second copy would fight over the shortcut.
        using var single = new Mutex(true, @"Local\io.github.sayre4ux.typethru", out var first);
        if (!first)
        {
            // An update starts the new copy while the old one quits; give it a moment.
            bool acquired;
            try { acquired = single.WaitOne(TimeSpan.FromSeconds(5)); }
            catch (AbandonedMutexException) { acquired = true; }
            if (!acquired)
            {
                MessageBox.Show("TypeThru is already running. Look for its icon in the notification area, next to the clock.", "TypeThru");
                return;
            }
        }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
#pragma warning disable WPF0001 // Fluent theme: follows light and dark mode, with Mica on Windows 11.
        app.ThemeMode = ThemeMode.System;
#pragma warning restore WPF0001
        var controller = new Controller();
        app.Startup += (_, _) => controller.Start();
        app.Run();
        // Free the shortcut and the lock before quitting, so an updated copy can start at once.
        controller.Dispose();
        single.ReleaseMutex();
    }
}
