// Draws the disk image window background at 1x and 2x. Run through package.sh.
// Layout in points, top left origin as Finder uses: the app at (190, 215), Applications at (490, 215).
// The hint ends above y 375, so a Finder path bar does not cover it.
import AppKit

let output = CommandLine.arguments[1]
let width: CGFloat = 680, height: CGFloat = 420

func text(_ string: String, size: CGFloat, weight: NSFont.Weight, alpha: CGFloat, centeredAt y: CGFloat) {
    let style = NSMutableParagraphStyle()
    style.alignment = .center
    let attributed = NSAttributedString(string: string, attributes: [
        .font: NSFont.systemFont(ofSize: size, weight: weight), .paragraphStyle: style,
        .foregroundColor: NSColor(red: 0.08, green: 0.09, blue: 0.14, alpha: alpha)])
    let box = attributed.boundingRect(with: NSSize(width: width - 80, height: 200), options: .usesLineFragmentOrigin)
    attributed.draw(with: NSRect(x: 40, y: height - y - box.height / 2, width: width - 80, height: box.height),
                    options: .usesLineFragmentOrigin)
}

func render(scale: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(width) * scale, pixelsHigh: Int(height) * scale,
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: width, height: height)
    NSGraphicsContext.saveGraphicsState()
    let graphics = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = graphics
    let canvas = NSRect(x: 0, y: 0, width: width, height: height)

    // DECISION: light, because Finder cannot switch a window background with the system appearance.
    NSGradient(starting: NSColor(red: 0.97, green: 0.98, blue: 1.00, alpha: 1),
               ending: NSColor(red: 0.89, green: 0.91, blue: 0.96, alpha: 1))!.draw(in: canvas, angle: -90)
    for (color, center, radius) in [(Brand.cyan, NSPoint(x: 120, y: 330), 320.0), (Brand.violet, NSPoint(x: 600, y: 80), 360.0)] {
        NSGradient(colors: [color.withAlphaComponent(0.22), .clear])!
            .draw(fromCenter: center, radius: 0, toCenter: center, radius: radius, options: [])
    }

    // Frosted card under the icons. DECISION: mid-tone, because Finder draws the icon names
    // black in light mode and white in dark mode, and both must stay readable on it.
    let card = NSBezierPath(roundedRect: NSRect(x: 40, y: height - 320, width: width - 80, height: 220), xRadius: 26, yRadius: 26)
    NSGradient(starting: NSColor(red: 0.45, green: 0.50, blue: 0.64, alpha: 0.92),
               ending: NSColor(red: 0.38, green: 0.43, blue: 0.58, alpha: 0.92))!.draw(in: card, angle: -90)
    NSGraphicsContext.saveGraphicsState()
    card.addClip()
    NSGradient(starting: NSColor.white.withAlphaComponent(0.16), ending: .clear)!
        .draw(in: NSRect(x: 40, y: height - 210, width: width - 80, height: 110), angle: -90)
    NSGraphicsContext.restoreGraphicsState()
    card.lineWidth = 1
    NSColor.white.withAlphaComponent(0.35).setStroke()
    card.stroke()

    // The TypeThru mark between the two icons, pointing to Applications.
    let art = NSRect(x: 280, y: height - 215 - 60, width: 120, height: 120)
    let keys = Brand.keystrokes(in: art)
    keys.lineWidth = 7
    NSColor.white.withAlphaComponent(0.6).setStroke()
    keys.stroke()
    let arrow = Brand.arrow(in: art)
    arrow.lineWidth = 9
    let glow = NSShadow()
    glow.shadowColor = Brand.cyan.withAlphaComponent(0.9)
    glow.shadowBlurRadius = 10
    NSGraphicsContext.saveGraphicsState()
    glow.set()
    Brand.cyan.setStroke()
    arrow.stroke()
    NSGraphicsContext.restoreGraphicsState()
    let outline = NSBezierPath(cgPath: arrow.cgPath.copy(strokingWithWidth: 9, lineCap: .round, lineJoin: .round, miterLimit: 10))
    NSGradient(starting: Brand.cyan, ending: Brand.violet)!.draw(in: outline, angle: 0)

    text("Drag TypeThru to Applications", size: 22, weight: .semibold, alpha: 0.9, centeredAt: 58)
    text("Opening it for the first time? If macOS cannot verify TypeThru, choose Open Anyway\nin System Settings › Privacy & Security. This is needed once.",
         size: 11.5, weight: .regular, alpha: 0.6, centeredAt: 354)
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

try render(scale: 1).write(to: URL(fileURLWithPath: output).appendingPathComponent("background.png"))
try render(scale: 2).write(to: URL(fileURLWithPath: output).appendingPathComponent("background@2x.png"))
