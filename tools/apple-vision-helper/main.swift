import CoreGraphics
import CoreImage
import Foundation
import Vision

private enum HelperError: Error, CustomStringConvertible {
    case usage(String)
    case unsupportedRevision(Int)
    case loadFailed(String)
    case noObservation
    case noInstances
    case writeFailed(String)

    var description: String {
        switch self {
        case .usage(let message): return message
        case .unsupportedRevision(let revision): return "unsupported Vision revision: \(revision)"
        case .loadFailed(let path): return "failed to load input image: \(path)"
        case .noObservation: return "Vision returned no foreground mask observation"
        case .noInstances: return "Vision returned no foreground instances"
        case .writeFailed(let path): return "failed to write mask PNG: \(path)"
        }
    }
}

private struct Arguments {
    let input: String
    let output: String
    let revision: Int
}

private func parseArguments(_ arguments: [String]) throws -> Arguments {
    guard arguments.first == "foreground-mask" else {
        throw HelperError.usage("usage: perfectpixel-vision-helper foreground-mask --input <absolute> --output <absolute> --revision 1")
    }
    var values: [String: String] = [:]
    var index = 1
    while index < arguments.count {
        let key = arguments[index]
        guard key.hasPrefix("--"), index + 1 < arguments.count else {
            throw HelperError.usage("every option requires one value")
        }
        guard values[key] == nil else {
            throw HelperError.usage("duplicate option: \(key)")
        }
        values[key] = arguments[index + 1]
        index += 2
    }
    guard values.count == 3,
          let input = values["--input"],
          let output = values["--output"],
          let revisionText = values["--revision"],
          let revision = Int(revisionText),
          input.hasPrefix("/"), output.hasPrefix("/") else {
        throw HelperError.usage("--input, --output and --revision are required; paths must be absolute")
    }
    return Arguments(input: input, output: output, revision: revision)
}

@available(macOS 14.0, *)
private func foregroundMask(_ arguments: Arguments) throws {
    guard arguments.revision == 1 else {
        throw HelperError.unsupportedRevision(arguments.revision)
    }
    let inputURL = URL(fileURLWithPath: arguments.input)
    let outputURL = URL(fileURLWithPath: arguments.output)
    guard let image = CIImage(contentsOf: inputURL) else {
        throw HelperError.loadFailed(arguments.input)
    }

    let request = VNGenerateForegroundInstanceMaskRequest()
    request.revision = VNGenerateForegroundInstanceMaskRequestRevision1
    let handler = VNImageRequestHandler(ciImage: image, options: [:])
    try handler.perform([request])
    guard let observation = request.results?.first else {
        throw HelperError.noObservation
    }
    guard !observation.allInstances.isEmpty else {
        throw HelperError.noInstances
    }

    let buffer = try observation.generateScaledMaskForImage(
        forInstances: observation.allInstances,
        from: handler
    )
    let mask = CIImage(cvPixelBuffer: buffer)
    let context = CIContext(options: [
        .cacheIntermediates: false,
        .useSoftwareRenderer: false,
    ])
    do {
        try context.writePNGRepresentation(
            of: mask,
            to: outputURL,
            format: .L8,
            colorSpace: CGColorSpaceCreateDeviceGray()
        )
    } catch {
        throw HelperError.writeFailed(arguments.output)
    }

    let version = ProcessInfo.processInfo.operatingSystemVersion
    let receipt: [String: Any] = [
        "ok": true,
        "backend": "AppleVision.VNGenerateForegroundInstanceMaskRequest",
        "revision": arguments.revision,
        "os": [
            "major": version.majorVersion,
            "minor": version.minorVersion,
            "patch": version.patchVersion,
        ],
        "instances": observation.allInstances.count,
    ]
    let data = try JSONSerialization.data(withJSONObject: receipt, options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

@main
private enum PerfectPixelVisionHelper {
    static func main() {
        do {
            let arguments = try parseArguments(Array(CommandLine.arguments.dropFirst()))
            guard #available(macOS 14.0, *) else {
                throw HelperError.usage("Apple Vision foreground instance masks require macOS 14.0+")
            }
            try foregroundMask(arguments)
        } catch {
            let message = "perfectpixel-vision-helper: \(error)\n"
            FileHandle.standardError.write(Data(message.utf8))
            Foundation.exit(2)
        }
    }
}
