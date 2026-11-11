using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Runtime.InteropServices.WindowsRuntime;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Windows.Graphics.Imaging;
using Windows.Media.Ocr;
using PixelFormat = System.Drawing.Imaging.PixelFormat;
using Rectangle = System.Drawing.Rectangle;

namespace TypeThru;

public abstract record CopyOutcome
{
    public sealed record Copied(int Lines, int Characters) : CopyOutcome;
    public sealed record Cancelled : CopyOutcome;
    public sealed record NoText : CopyOutcome;
    public sealed record Failed(string Reason) : CopyOutcome;
}

/// <summary>
/// Copy Text from Screen: the user drags over an area, Windows' own text recognition reads it on
/// this PC, and only the text is put on the clipboard. The picture stays in memory and is
/// discarded right away; nothing is saved.
/// </summary>
internal static class ScreenCopy
{
    public static async Task<CopyOutcome> Copy()
    {
        if (await Select() is not { } area) return new CopyOutcome.Cancelled();
        var engine = OcrEngine.TryCreateFromUserProfileLanguages();
        if (engine is null)
            return new CopyOutcome.Failed("Windows has no text recognition for your languages. Add one in Settings › Time & language › Language & region, under the language's options.");
        // Let the selection window disappear before the picture is taken.
        await Task.Delay(150);
        string text;
        using (var picture = Capture(area))
        {
            // DECISION: small text on a 100% scale screen reads much better at twice the size, as on the Mac.
            var scale = Native.ScaleAt(new Native.Point { X = area.X + area.Width / 2, Y = area.Y + area.Height / 2 });
            var enlarge = scale < 1.5 && Math.Max(area.Width, area.Height) * 2 <= OcrEngine.MaxImageDimension;
            using var input = enlarge ? Enlarged(picture) : picture.Clone(new Rectangle(0, 0, picture.Width, picture.Height), PixelFormat.Format32bppPArgb);
            using var bitmap = ToSoftwareBitmap(input);
            var result = await engine.RecognizeAsync(bitmap);
            var pieces = new List<Piece>();
            foreach (var line in result.Lines)
            {
                double left = double.MaxValue, top = double.MaxValue, right = 0, bottom = 0;
                foreach (var word in line.Words)
                {
                    left = Math.Min(left, word.BoundingRect.X);
                    top = Math.Min(top, word.BoundingRect.Y);
                    right = Math.Max(right, word.BoundingRect.X + word.BoundingRect.Width);
                    bottom = Math.Max(bottom, word.BoundingRect.Y + word.BoundingRect.Height);
                }
                if (right > left) pieces.Add(new Piece(line.Text, left, top, right - left, bottom - top));
            }
            text = ScreenLayout.Layout(pieces);
        }
        if (text.Length == 0) return new CopyOutcome.NoText();
        SetClipboard(text);
        return new CopyOutcome.Copied(text.Split('\n').Length, TextRules.Elements(text).Count);
    }

    private static void SetClipboard(string text)
    {
        // Another app may hold the clipboard for a moment.
        for (var attempt = 0; ; attempt++)
        {
            try { System.Windows.Clipboard.SetText(text); return; }
            catch (System.Runtime.InteropServices.COMException) when (attempt < 5) { System.Threading.Thread.Sleep(50); }
        }
    }

    private static Bitmap Capture(Rectangle area)
    {
        var picture = new Bitmap(area.Width, area.Height, PixelFormat.Format32bppPArgb);
        using var graphics = Graphics.FromImage(picture);
        graphics.CopyFromScreen(area.X, area.Y, 0, 0, area.Size, CopyPixelOperation.SourceCopy);
        return picture;
    }

    private static Bitmap Enlarged(Bitmap picture)
    {
        var larger = new Bitmap(picture.Width * 2, picture.Height * 2, PixelFormat.Format32bppPArgb);
        using var graphics = Graphics.FromImage(larger);
        graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
        graphics.DrawImage(picture, 0, 0, larger.Width, larger.Height);
        return larger;
    }

    private static SoftwareBitmap ToSoftwareBitmap(Bitmap picture)
    {
        var data = picture.LockBits(new Rectangle(0, 0, picture.Width, picture.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppPArgb);
        try
        {
            var bytes = new byte[data.Stride * data.Height];
            System.Runtime.InteropServices.Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
            return SoftwareBitmap.CreateCopyFromBuffer(bytes.AsBuffer(), BitmapPixelFormat.Bgra8, picture.Width, picture.Height, BitmapAlphaMode.Premultiplied);
        }
        finally { picture.UnlockBits(data); }
    }

    /// <summary>A dimmed window over every screen; drag to select, Esc cancels. Returns the area in screen pixels.</summary>
    private static Task<Rectangle?> Select()
    {
        var done = new TaskCompletionSource<Rectangle?>();
        var canvas = new Canvas { Background = new SolidColorBrush(System.Windows.Media.Color.FromArgb(70, 0, 0, 0)) };
        var box = new System.Windows.Shapes.Rectangle
        {
            Stroke = System.Windows.Media.Brushes.White, StrokeThickness = 1.5, Fill = new SolidColorBrush(System.Windows.Media.Color.FromArgb(40, 255, 255, 255)),
            Visibility = Visibility.Collapsed,
        };
        canvas.Children.Add(box);
        canvas.Children.Add(new TextBlock
        {
            Text = "Drag over the text to copy. Esc cancels.", Foreground = System.Windows.Media.Brushes.White, FontSize = 15,
            Margin = new Thickness(24), Effect = new System.Windows.Media.Effects.DropShadowEffect { BlurRadius = 6, ShadowDepth = 0 },
        });
        var window = new Window
        {
            WindowStyle = WindowStyle.None, AllowsTransparency = true, Background = System.Windows.Media.Brushes.Transparent, Topmost = true,
            ShowInTaskbar = false, ResizeMode = ResizeMode.NoResize, Cursor = Cursors.Cross, Content = canvas,
            Left = SystemParameters.VirtualScreenLeft, Top = SystemParameters.VirtualScreenTop,
            Width = SystemParameters.VirtualScreenWidth, Height = SystemParameters.VirtualScreenHeight,
        };
        System.Windows.Point? start = null;
        Native.Point startPixel = default;
        void Finish(Rectangle? area) { if (done.TrySetResult(area)) window.Close(); }
        window.KeyDown += (_, e) => { if (e.Key == Key.Escape) Finish(null); };
        window.Deactivated += (_, _) => Finish(null);
        window.MouseLeftButtonDown += (_, e) =>
        {
            start = e.GetPosition(canvas);
            Native.GetCursorPos(out startPixel);
            window.CaptureMouse();
        };
        window.MouseMove += (_, e) =>
        {
            if (start is not { } from) return;
            var to = e.GetPosition(canvas);
            Canvas.SetLeft(box, Math.Min(from.X, to.X));
            Canvas.SetTop(box, Math.Min(from.Y, to.Y));
            box.Width = Math.Abs(to.X - from.X);
            box.Height = Math.Abs(to.Y - from.Y);
            box.Visibility = Visibility.Visible;
        };
        window.MouseLeftButtonUp += (_, _) =>
        {
            if (start is null) return;
            window.ReleaseMouseCapture();
            // Screen pixels from the cursor itself, so the area is right on screens of any scale.
            Native.GetCursorPos(out var end);
            var area = Rectangle.FromLTRB(Math.Min(startPixel.X, end.X), Math.Min(startPixel.Y, end.Y), Math.Max(startPixel.X, end.X), Math.Max(startPixel.Y, end.Y));
            Finish(area.Width >= 4 && area.Height >= 4 ? area : null);
        };
        window.Show();
        window.Activate();
        return done.Task;
    }
}
