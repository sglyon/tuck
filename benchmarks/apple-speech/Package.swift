// swift-tools-version: 6.2
import PackageDescription
import Foundation

let localSource = ProcessInfo.processInfo.environment["TUCK_FLUIDAUDIO_SOURCE"]
let fluidAudio: Package.Dependency = localSource.map {
    .package(name: "FluidAudio", path: $0, traits: [])
} ?? .package(url: "https://github.com/FluidInference/FluidAudio.git",
             revision: "503b4bd1bbf7220882de39fe8ae6716aae4132da", traits: [])

let package = Package(
    name: "AppleSpeechBenchmark",
    platforms: [.macOS(.v14)],
    dependencies: [
        fluidAudio,
    ],
    targets: [
        .executableTarget(name: "AppleSpeechBenchmark", dependencies: [
            .product(name: "FluidAudio", package: "FluidAudio"),
        ]),
    ]
)
