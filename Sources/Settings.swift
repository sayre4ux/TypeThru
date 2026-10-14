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
                Toggle("Type line breaks as Shift+Return", isOn: binding(\.shiftReturn))
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
                        Button(model.installing ? "Installing…" : "Install") { model.perform?(.installUpdate) }
                            .prominentGlass().tint(Brand.accent).disabled(model.installing)
                    }
                    Button("Check Now") { model.perform?(.checkForUpdates) }.disabled(model.installing)
                }
            }

            Section("Help") {
                HStack {
                    Button("Diagnostics") { model.perform?(.diagnostics) }
                    Button("Check Accessibility") { model.perform?(.checkAccessibility) }
                }
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .background(GlassBackground().ignoresSafeArea())
        .frame(width: 460)
        .fixedSize(horizontal: false, vertical: true)
    }

    @ViewBuilder private func note(_ text: String?) -> some View {
        if let text {
            Text(text).font(.caption).foregroundStyle(.orange).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func binding<T>(_ keyPath: ReferenceWritableKeyPath<PanelModel, T>) -> Binding<T> { model.binding(keyPath) }
}

/// Liquid Glass on macOS 26 and later, with the panel's dark tint; a blurred material before that.
private struct GlassBackground: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        if #available(macOS 26, *) {
            let glass = NSGlassEffectView()
            glass.tintColor = NSColor.black.withAlphaComponent(0.45)
            return glass
        }
        let material = NSVisualEffectView()
        material.material = .hudWindow
        material.blendingMode = .behindWindow
        material.state = .active
        return material
    }

    func updateNSView(_ view: NSView, context: Context) {}
}

final class SettingsWindowController: NSObject, NSWindowDelegate {
    var onClose: (() -> Void)?
    private var window: NSWindow?

    func show(model: PanelModel) {
        if window == nil {
            let window = NSWindow(contentViewController: NSHostingController(rootView: SettingsView(model: model)))
            window.title = "TypeThru Settings"
            window.styleMask = [.titled, .closable, .fullSizeContentView]
            window.titlebarAppearsTransparent = true
            window.appearance = NSAppearance(named: .darkAqua)
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
