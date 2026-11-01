import AppKit
import SwiftUI

// Settings window for options people change rarely. It shares the panel's model.

struct SettingsView: View {
    @ObservedObject var model: PanelModel

    var body: some View {
        Form {
            Section("General") {
                Toggle("Open TypeThru at login", isOn: binding(\.launchAtLogin))
                note(model.launchNote)
                LabeledContent("Shortcut") {
                    HStack(spacing: 6) {
                        Text(model.recording ? "Press keys…" : model.shortcut)
                            .font(.system(.body, design: .rounded).weight(.medium))
                            .padding(.horizontal, 10).padding(.vertical, 3)
                            .background(.quaternary, in: Capsule())
                        Button(model.recording ? "Cancel" : "Change") { model.perform?(.recordShortcut) }
                        if !model.shortcutIsStandard && !model.recording {
                            Button("Reset") { model.perform?(.resetShortcut) }
                        }
                    }
                }
                note(model.shortcutNote)
            }

            Section("Typing") {
                Picker("Method", selection: binding(\.method)) {
                    ForEach(TypingMethod.allCases, id: \.self) { Text($0.shortTitle).tag($0) }
                }
                Picker("Speed", selection: binding(\.speedIndex)) {
                    ForEach(model.speeds.indices, id: \.self) { Text(model.speeds[$0]).tag($0) }
                }
                Toggle("Replace symbols without a key (• → -)", isOn: binding(\.replaceSymbols))
                Picker("Markdown (**bold**, *italic*)", selection: binding(\.markdown)) {
                    ForEach(MarkdownMode.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                Toggle("Type line breaks as Shift+Return", isOn: binding(\.shiftReturn))
            }

            Section("Copy Text from Screen") {
                Toggle("Copy text from the screen with \(model.copyShortcut)", isOn: binding(\.copyFromScreen))
                Text("Select an area, or press Space to pick a window. Its text is read on this Mac and copied, and the picture is deleted at once. macOS asks once for Screen Recording permission.")
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }

            Section("Virtual Keyboard") {
                LabeledContent("Status", value: model.status.text)
                HStack {
                    Button("Set Up…") { model.perform?(.setUp) }
                    Button("Check") { model.perform?(.checkVirtualKeyboard) }
                    Spacer()
                    Button("Uninstall TypeThru…") { model.perform?(.uninstall) }
                }
            }

            Section("Updates") {
                Toggle("Check for updates automatically", isOn: binding(\.checkUpdates))
                LabeledContent("Version", value: Updates.currentVersion)
                HStack {
                    Text(model.updateStatus).foregroundStyle(.secondary)
                    Spacer()
                    if model.update != nil {
                        Button("What's New") { model.perform?(.updateNotes) }.disabled(model.installing)
                        Button(model.installing ? "Installing…" : "Install") { model.perform?(.installUpdate) }
                            .prominentGlass().tint(Brand.accent).disabled(model.installing)
                    }
                    Button("Check Now") { model.perform?(.checkForUpdates) }.disabled(model.installing)
                }
            }

            Section("Help") {
                HStack {
                    Button("What's New") { model.perform?(.whatsNew) }
                    Button("Diagnostics") { model.perform?(.diagnostics) }
                    Button("Check Accessibility") { model.perform?(.checkAccessibility) }
                }
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .frame(width: 460)
    }

    @ViewBuilder private func note(_ text: String?) -> some View {
        if let text {
            Text(text).font(.caption).foregroundStyle(.orange).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func binding<T>(_ keyPath: ReferenceWritableKeyPath<PanelModel, T>) -> Binding<T> { model.binding(keyPath) }
}

/// Liquid Glass on macOS 26 and later, with the panel's tint; a blurred material before that.
/// The window's whole background, title bar included, with `content` below the title bar.
/// DECISION: glass as the window's content view rather than a SwiftUI background, which left the
/// title bar fully transparent; macOS passes clicks on fully transparent parts of a window through
/// to the window behind, close button included.
func glassWindowContent(around content: NSView, size: NSSize, titleBar: CGFloat) -> NSView {
    let container = NSView(frame: NSRect(origin: .zero, size: size))
    let background: NSView
    if #available(macOS 26, *) {
        let glass = TintedGlass()
        glass.tintColor = Brand.glassTint
        background = glass
    } else {
        let material = NSVisualEffectView()
        material.material = .hudWindow
        material.blendingMode = .behindWindow
        material.state = .active
        background = material
    }
    background.frame = container.bounds
    content.frame = NSRect(x: 0, y: 0, width: size.width, height: size.height - titleBar)
    for view in [background, content] {
        view.autoresizingMask = [.width, .height]
        container.addSubview(view)
    }
    return container
}

/// Resolves the tint again when the system switches between light and dark mode.
@available(macOS 26, *)
private final class TintedGlass: NSGlassEffectView {
    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        effectiveAppearance.performAsCurrentDrawingAppearance { tintColor = NSColor(cgColor: Brand.glassTint.cgColor) }
    }
}

final class SettingsWindowController: NSObject, NSWindowDelegate {
    var onClose: (() -> Void)?
    private var window: NSWindow?

    func show(model: PanelModel) {
        if window == nil {
            // The full height of the form, measured once; on a small screen the window is shorter and the form scrolls.
            let content = NSHostingView(rootView: SettingsView(model: model).fixedSize(horizontal: false, vertical: true)).fittingSize
            let host = NSHostingView(rootView: SettingsView(model: model))
            host.sizingOptions = []
            let visible = (NSScreen.main ?? NSScreen.screens[0]).visibleFrame
            let window = NSWindow(contentRect: NSRect(origin: .zero, size: content),
                                  styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: false)
            let titleBar = window.frame.height - window.contentLayoutRect.height
            let size = NSSize(width: content.width, height: min(content.height + titleBar, visible.height - 40))
            window.setContentSize(size)
            window.contentView = glassWindowContent(around: host, size: size, titleBar: titleBar)
            window.title = "TypeThru Settings"
            window.titlebarAppearsTransparent = true
            // The window is clear, so the glass behind the form shows the desktop like the panel.
            window.isOpaque = false
            window.backgroundColor = .clear
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.center()
            self.window = window
        }
        // Settings is a normal window, so TypeThru comes to the front while it is open.
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
    }

    func windowWillClose(_ notification: Notification) { onClose?() }
}
