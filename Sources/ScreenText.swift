import AppKit
import Vision

/// Copy Text from Screen: the user selects an area with macOS's own screenshot tool, the text in
/// it is recognized on this Mac, and only that text is put on the clipboard. The picture is
/// deleted as soon as it is read and never leaves the Mac.
enum ScreenText {
    enum Outcome: Equatable {
        case copied(lines: Int, characters: Int), cancelled, noText, needsPermission, failed(String)
    }

    struct Line { let text: String; let box: CGRect }

    /// Calls back on the main thread.
    static func copy(completion: @escaping (Outcome) -> Void) {
        guard CGPreflightScreenCaptureAccess() else { completion(.needsPermission); return }
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result { try select() }
            DispatchQueue.main.async {
                switch result {
                case .failure(let error): completion(.failed(error.localizedDescription))
                case .success(nil): completion(.cancelled)
                case .success(let lines?):
                    let text = layout(lines)
                    guard !text.isEmpty else { completion(.noText); return }
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                    completion(.copied(lines: text.split(separator: "\n", omittingEmptySubsequences: false).count,
                                       characters: text.count))
                }
            }
        }
    }

    /// Runs the screenshot selection and recognizes its text. Nil when the user cancels.
    private static func select() throws -> [Line]? {
        let manager = FileManager.default
        // DECISION: a fresh folder in this user's private temporary folder, removed right after.
        let folder = try manager.url(for: .itemReplacementDirectory, in: .userDomainMask,
                                     appropriateFor: manager.temporaryDirectory, create: true)
        defer { try? manager.removeItem(at: folder) }
        let picture = folder.appendingPathComponent("selection.png")
        let tool = Process()
        tool.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        // -i: drag an area, or Space to pick a window, Esc cancels. -x: no sound. -o: no window shadow.
        tool.arguments = ["-i", "-x", "-o", picture.path]
        try tool.run()
        tool.waitUntilExit()
        guard manager.fileExists(atPath: picture.path) else { return nil }
        guard let source = CGImageSourceCreateWithURL(picture as CFURL, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw Updates.Failure("The selected area could not be read.")
        }
        // The first recognition after a restart can take a while; say so if it does.
        var done = false
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { if !done { Toast.show("Reading text…", symbol: "text.viewfinder") } }
        defer { DispatchQueue.main.async { done = true } }
        // Screenshots of a Retina screen are saved at 144 dpi, others at 72.
        let dpi = (CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])?[kCGImagePropertyDPIWidth] as? Double ?? 144
        return try recognize(dpi < 100 ? enlarged(image) : image)
    }

    // DECISION: small text from a non-Retina screen reads much better at twice the size (f was
    // read as t at 1x). Retina pictures are already that size; enlarging further made it worse.
    static func enlarged(_ image: CGImage) -> CGImage {
        guard image.width * image.height <= 20_000_000,
              let context = CGContext(data: nil, width: image.width * 2, height: image.height * 2, bitsPerComponent: 8,
                                      bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return image }
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width * 2, height: image.height * 2))
        return context.makeImage() ?? image
    }

    static func recognize(_ image: CGImage) throws -> [Line] {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.automaticallyDetectsLanguage = true
        // DECISION: language correction on; without it words ran together ("applyingthe").
        request.usesLanguageCorrection = true
        try VNImageRequestHandler(cgImage: image).perform([request])
        return (request.results ?? []).compactMap { observation in
            observation.topCandidates(1).first.map { Line(text: $0.string, box: observation.boundingBox) }
        }
    }

    /// Puts recognized pieces back in reading order: rows top to bottom, a blank line where a
    /// paragraph gap was, and a tab between pieces far apart on one row (table columns).
    /// Boxes are in Vision's coordinates: 0 to 1, origin at the bottom left.
    static func layout(_ pieces: [Line]) -> String {
        var rows: [[Line]] = []
        for piece in pieces.filter({ !$0.text.isEmpty }).sorted(by: { $0.box.midY > $1.box.midY }) {
            if let last = rows.last?.first, abs(last.box.midY - piece.box.midY) < min(last.box.height, piece.box.height) / 2 {
                rows[rows.count - 1].append(piece)
            } else {
                rows.append([piece])
            }
        }
        var text = "", previous: CGRect?
        for row in rows {
            let sorted = row.sorted { $0.box.minX < $1.box.minX }
            let height = sorted.map(\.box.height).max() ?? 0
            if let previous {
                text += "\n"
                if previous.minY - (sorted.map(\.box.maxY).max() ?? 0) > height * 0.9 { text += "\n" }
            }
            for (index, piece) in sorted.enumerated() {
                if index > 0 { text += piece.box.minX - sorted[index - 1].box.maxX > height * 1.5 ? "\t" : " " }
                text += piece.text
            }
            previous = CGRect(x: 0, y: sorted.map(\.box.minY).min() ?? 0, width: 1, height: height)
        }
        return text
    }
}
