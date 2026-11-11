using System;
using System.Runtime.InteropServices;

namespace TypeThru;

/// <summary>
/// The Windows calls TypeThru uses. DECISION: no keyboard hooks and no raw input (the privacy
/// contract); key state is only polled with GetAsyncKeyState while TypeThru waits or types.
/// </summary>
internal static class Native
{
    public const uint InputKeyboard = 1;
    public const uint KeyUp = 0x0002, ScanCode = 0x0008, Unicode = 0x0004, ExtendedKey = 0x0001;
    public const int VkShift = 0x10, VkControl = 0x11, VkMenu = 0x12, VkEscape = 0x1B, VkReturn = 0x0D, VkTab = 0x09;
    public const int VkLButton = 0x01, VkRButton = 0x02, VkMButton = 0x04, VkLWin = 0x5B, VkRWin = 0x5C;
    public const uint MapVkToVscEx = 4, MapVkToChar = 2;
    public const int WmHotKey = 0x0312;
    public const uint ModNoRepeat = 0x4000;

    [StructLayout(LayoutKind.Sequential)]
    public struct KeyboardInput
    {
        public ushort Vk;
        public ushort Scan;
        public uint Flags;
        public uint Time;
        public IntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MouseInput
    {
        public int Dx, Dy;
        public uint MouseData, Flags, Time;
        public IntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct InputUnion
    {
        [FieldOffset(0)] public MouseInput Mouse;
        [FieldOffset(0)] public KeyboardInput Keyboard;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct Input
    {
        public uint Type;
        public InputUnion Data;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct Point { public int X, Y; }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint count, Input[] inputs, int size);

    [DllImport("user32.dll")]
    public static extern short VkKeyScanExW(char ch, IntPtr layout);

    [DllImport("user32.dll")]
    public static extern uint MapVirtualKeyExW(uint code, uint mapType, IntPtr layout);

    [DllImport("user32.dll")]
    public static extern IntPtr GetKeyboardLayout(uint thread);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);

    [DllImport("user32.dll")]
    public static extern short GetAsyncKeyState(int key);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool RegisterHotKey(IntPtr window, int id, uint modifiers, uint key);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool UnregisterHotKey(IntPtr window, int id);

    [DllImport("user32.dll")]
    public static extern IntPtr MonitorFromPoint(Point point, uint flags);

    [DllImport("shcore.dll")]
    public static extern int GetDpiForMonitor(IntPtr monitor, int type, out uint dpiX, out uint dpiY);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr OpenProcess(uint access, [MarshalAs(UnmanagedType.Bool)] bool inherit, uint process);

    [DllImport("kernel32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool CloseHandle(IntPtr handle);

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetTokenInformation(IntPtr token, int infoClass, out int info, int length, out int returned);

    public static bool IsDown(int key) => (GetAsyncKeyState(key) & 0x8000) != 0;

    /// <summary>Whether the process runs as administrator. Windows lets a normal app send keys only to normal windows.</summary>
    public static bool IsElevated(uint process)
    {
        const uint QueryLimited = 0x1000, TokenQuery = 0x0008;
        const int TokenElevation = 20;
        var handle = OpenProcess(QueryLimited, false, process);
        if (handle == IntPtr.Zero) return false;
        try
        {
            if (!OpenProcessToken(handle, TokenQuery, out var token)) return false;
            try { return GetTokenInformation(token, TokenElevation, out var elevated, sizeof(int), out _) && elevated != 0; }
            finally { CloseHandle(token); }
        }
        finally { CloseHandle(handle); }
    }

    /// <summary>The display scale where the point is, 1.0 at 96 dpi.</summary>
    public static double ScaleAt(Point point)
    {
        const uint Nearest = 2;
        return GetDpiForMonitor(MonitorFromPoint(point, Nearest), 0, out var dpi, out _) == 0 ? dpi / 96.0 : 1.0;
    }
}
