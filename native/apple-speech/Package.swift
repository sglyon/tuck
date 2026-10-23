// swift-tools-version: 6.2
import Foundation
import PackageDescription

let localSource = ProcessInfo.processInfo.environment["TUCK_FLUIDAUDIO_SOURCE"]
let dependency: Package.Dependency = localSource.map {
    .package(name: "FluidAudio", path: $0, traits: [])
} ?? .package(url: "https://github.com/FluidInference/FluidAudio.git",
             revision: "503b4bd1bbf7220882de39fe8ae6716aae4132da", traits: [])

let package = Package(
    name: "TuckSpeech",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "tuck-speech", targets: ["TuckSpeech"])],
    dependencies: [dependency],
    targets: [.executableTarget(name: "TuckSpeech", dependencies: [
        .product(name: "FluidAudio", package: "FluidAudio"),
    ])]
)
