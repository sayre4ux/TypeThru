import AppKit
import SwiftUI

/// Release notes from CHANGELOG.md, which build.sh copies into the app. After an update the app
/// shows every version since the one the user last ran, without asking the network.
enum Changelog {
    struct Entry: Identifiable, Equatable {
        let version: String
        let title: String
        let lines: [String]
        var id: String { version }
    }

    /// Each "## " heading starts a version. Indented lines continue the line above.
    static func parse(_ text: String) -> [Entry] {
        var entries: [Entry] = [], title: String?, lines: [String] = []
        func finish() {
            guard let title else { return }
            let version = Updates.numbers(title).map(String.init).joined(separator: ".")
            if !version.isEmpty { entries.append(Entry(version: version, title: title, lines: lines)) }
        }
        for line in text.components(separatedBy: .newlines) {
            if line.hasPrefix("## ") {
                finish()
                title = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                lines = []
            } else if title != nil, !line.trimmingCharacters(in: .whitespaces).isEmpty {
                if line.hasPrefix(" "), let last = lines.popLast() {
                    lines.append(last + " " + line.trimmingCharacters(in: .whitespaces))
                } else {
                    lines.append(line)
                }
            }
        }
        finish()
        return entries
    }

    /// Versions after `last` up to `current`, newest first.
    static func since(_ last: String, upTo current: String, in entries: [Entry]) -> [Entry] {
        entries.filter { Updates.isNewer($0.version, than: last) && !Updates.isNewer($0.version, than: current) }
    }

    static var bundled: [Entry] {
        guard let url = Bundle.main.url(forResource: "CHANGELOG", withExtension: "md"),
              let text = try? String(contentsOf: url, encoding: .utf8) else { return [] }
        return parse(text)
    }
}

struct WhatsNewView: View {
    let entries: [Changelog.Entry]
    var measuring = false
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // A scroll view has no natural height, so the window is measured without one.
            if measuring { notes } else { ScrollView { notes } }
            HStack {
                Spacer()
                Button("Continue", action: close).keyboardShortcut(.defaultAction).prominentGlass().tint(Brand.accent)
            }
            .padding(16)
        }
        .frame(width: 480)
    }

    private var notes: some View {
        VStack(alignment: .leading, spacing: 22) {
            ForEach(entries) { entry in
                VStack(alignment: .leading, spacing: 8) {
                    Text("TypeThru \(entry.title)").font(.title3.weight(.semibold))
                    ForEach(Array(entry.lines.enumerated()), id: \.offset) { _, line in row(line) }
                }
            }
        }
        .padding(.horizontal, 24).padding(.vertical, 20)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func row(_ line: String) -> some View {
        let bullet = line.hasPrefix("- ")
        let text = bullet ? String(line.dropFirst(2)) : line
        let styled = (try? AttributedString(markdown: text)) ?? AttributedString(text)
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            if bullet { Text("•").foregroundStyle(Brand.accent) }
            Text(styled).fixedSize(horizontal: false, vertical: true)
        }
        .font(.callout)
    }
}

final class WhatsNewWindowController: NSObject, NSWindowDelegate {
    private var window: NSWindow?

    func show(_ entries: [Changelog.Entry]) {
        guard !entries.isEmpty else { return }
        window?.close()
        let close = { [weak self] in self?.window?.close() }
        let content = NSHostingView(rootView: WhatsNewView(entries: entries, measuring: true, close: {})
            .fixedSize(horizontal: false, vertical: true)).fittingSize
        let host = NSHostingView(rootView: WhatsNewView(entries: entries, close: { close() }))
        host.sizingOptions = []
        let visible = (NSScreen.main ?? NSScreen.screens[0]).visibleFrame
        let window = NSWindow(contentRect: NSRect(origin: .zero, size: content),
                              styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: false)
        let titleBar = window.frame.height - window.contentLayoutRect.height
        let size = NSSize(width: content.width, height: min(content.height + titleBar, visible.height * 0.75))
        window.setContentSize(size)
        window.contentView = glassWindowContent(around: host, size: size, titleBar: titleBar)
        window.title = entries.count == 1 ? "What's New in TypeThru" : "What's New Since Your Last Version"
        window.titlebarAppearsTransparent = true
        window.isOpaque = false
        window.backgroundColor = .clear
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.center()
        self.window = window
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    func windowWillClose(_ notification: Notification) { window = nil }
}
