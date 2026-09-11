import CoreGraphics
import CoreText
import Foundation

// Usage: swift quartz_graphics.swift <output.pdf>
guard CommandLine.arguments.count > 1 else {
    FileHandle.standardError.write("usage: quartz_graphics.swift <output.pdf>\n".data(using: .utf8)!)
    exit(1)
}
let outputPath = CommandLine.arguments[1]

var mediaBox = CGRect(x: 0, y: 0, width: 595, height: 842) // A4-ish points

guard let consumer = CGDataConsumer(url: URL(fileURLWithPath: outputPath) as CFURL) else {
    fatalError("could not create data consumer")
}
guard let ctx = CGContext(consumer: consumer, mediaBox: &mediaBox, nil) else {
    fatalError("could not create PDF context")
}

let rgb = CGColorSpaceCreateDeviceRGB()
guard let cmyk = CGColorSpace(name: CGColorSpace.genericCMYK) else {
    fatalError("no CMYK color space")
}

func drawPage1() {
    ctx.beginPDFPage(nil)

    // --- RGB fills/strokes ---
    ctx.setFillColorSpace(rgb)
    ctx.setFillColor(red: 0.85, green: 0.15, blue: 0.15, alpha: 1.0)
    ctx.fill(CGRect(x: 40, y: 700, width: 120, height: 80))

    ctx.setStrokeColorSpace(rgb)
    ctx.setStrokeColor(red: 0.1, green: 0.3, blue: 0.9, alpha: 1.0)
    ctx.setLineWidth(6)
    ctx.stroke(CGRect(x: 180, y: 700, width: 120, height: 80))

    // --- CMYK fills/strokes ---
    let cmykFill: [CGFloat] = [0.0, 0.7, 0.9, 0.0, 1.0] // C M Y K A -> orange-ish
    if let cmykColor = CGColor(colorSpace: cmyk, components: cmykFill) {
        ctx.setFillColor(cmykColor)
        ctx.fillEllipse(in: CGRect(x: 340, y: 700, width: 100, height: 80))
    }

    let cmykStroke: [CGFloat] = [0.9, 0.0, 0.4, 0.1, 1.0] // teal-ish
    if let cmykStrokeColor = CGColor(colorSpace: cmyk, components: cmykStroke) {
        ctx.setStrokeColor(cmykStrokeColor)
        ctx.setLineWidth(5)
        ctx.strokeEllipse(in: CGRect(x: 460, y: 700, width: 100, height: 80))
    }

    // --- Red text via CoreText ---
    let text = "Quartz CMYK/RGB Test Page 1" as CFString
    let font = CTFontCreateWithName("Helvetica-Bold" as CFString, 20, nil)
    let attrs: [CFString: Any] = [
        kCTFontAttributeName: font,
        kCTForegroundColorAttributeName: CGColor(red: 0.8, green: 0.0, blue: 0.0, alpha: 1.0)
    ]
    let attrString = CFAttributedStringCreate(nil, text, attrs as CFDictionary)
    let line = CTLineCreateWithAttributedString(attrString!)
    ctx.textPosition = CGPoint(x: 40, y: 650)
    CTLineDraw(line, ctx)

    // --- Axial gradient ---
    let gradColors = [
        CGColor(red: 1.0, green: 0.0, blue: 0.0, alpha: 1.0),
        CGColor(red: 0.0, green: 0.0, blue: 1.0, alpha: 1.0)
    ] as CFArray
    if let axialGradient = CGGradient(colorsSpace: rgb, colors: gradColors, locations: [0.0, 1.0]) {
        ctx.saveGState()
        ctx.addRect(CGRect(x: 40, y: 540, width: 240, height: 70))
        ctx.clip()
        ctx.drawLinearGradient(
            axialGradient,
            start: CGPoint(x: 40, y: 540),
            end: CGPoint(x: 280, y: 610),
            options: []
        )
        ctx.restoreGState()
    }

    // --- Radial gradient ---
    let radColors = [
        CGColor(red: 1.0, green: 1.0, blue: 0.2, alpha: 1.0),
        CGColor(red: 0.0, green: 0.5, blue: 0.1, alpha: 1.0)
    ] as CFArray
    if let radialGradient = CGGradient(colorsSpace: rgb, colors: radColors, locations: [0.0, 1.0]) {
        ctx.saveGState()
        ctx.addEllipse(in: CGRect(x: 320, y: 540, width: 200, height: 70))
        ctx.clip()
        ctx.drawRadialGradient(
            radialGradient,
            startCenter: CGPoint(x: 420, y: 575),
            startRadius: 0,
            endCenter: CGPoint(x: 420, y: 575),
            endRadius: 100,
            options: []
        )
        ctx.restoreGState()
    }

    // --- Transparency layer with alpha 0.5 shapes ---
    ctx.saveGState()
    ctx.setAlpha(0.5)
    ctx.beginTransparencyLayer(auxiliaryInfo: nil)
    ctx.setFillColor(red: 1.0, green: 0.0, blue: 0.5, alpha: 1.0)
    ctx.fill(CGRect(x: 60, y: 420, width: 150, height: 90))
    ctx.setFillColor(red: 0.0, green: 0.8, blue: 0.8, alpha: 1.0)
    ctx.fill(CGRect(x: 160, y: 460, width: 150, height: 90))
    ctx.endTransparencyLayer()
    ctx.restoreGState()

    // --- Small RGB bitmap image ---
    let w = 64, h = 64
    var pixels = [UInt8](repeating: 0, count: w * h * 4)
    for y in 0..<h {
        for x in 0..<w {
            let idx = (y * w + x) * 4
            pixels[idx + 0] = UInt8((x * 255) / w)       // R
            pixels[idx + 1] = UInt8((y * 255) / h)       // G
            pixels[idx + 2] = UInt8(((x + y) * 255) / (w + h)) // B
            pixels[idx + 3] = 255                        // A
        }
    }
    let bitmapCtx = CGContext(
        data: &pixels,
        width: w,
        height: h,
        bitsPerComponent: 8,
        bytesPerRow: w * 4,
        space: rgb,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )
    if let image = bitmapCtx?.makeImage() {
        ctx.draw(image, in: CGRect(x: 400, y: 420, width: 130, height: 130))
    }

    ctx.endPDFPage()
}

func drawPage2() {
    ctx.beginPDFPage(nil)

    let text = "Page 2: more CMYK shapes" as CFString
    let font = CTFontCreateWithName("Helvetica-Bold" as CFString, 20, nil)
    let attrs: [CFString: Any] = [
        kCTFontAttributeName: font,
        kCTForegroundColorAttributeName: CGColor(red: 0.8, green: 0.0, blue: 0.0, alpha: 1.0)
    ]
    let attrString = CFAttributedStringCreate(nil, text, attrs as CFDictionary)
    let line = CTLineCreateWithAttributedString(attrString!)
    ctx.textPosition = CGPoint(x: 40, y: 780)
    CTLineDraw(line, ctx)

    let cmykSquares: [[CGFloat]] = [
        [1.0, 0.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 1.0, 1.0]
    ]
    for (i, comp) in cmykSquares.enumerated() {
        if let c = CGColor(colorSpace: cmyk, components: comp) {
            ctx.setFillColor(c)
            ctx.fill(CGRect(x: 40 + CGFloat(i) * 130, y: 650, width: 100, height: 100))
        }
    }

    ctx.endPDFPage()
}

drawPage1()
drawPage2()
ctx.closePDF()
