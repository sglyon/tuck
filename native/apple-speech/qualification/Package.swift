// swift-tools-version: 6.2
import Foundation
import PackageDescription
guard let source = ProcessInfo.processInfo.environment["TUCK_FLUIDAUDIO_SOURCE"] else {
    fatalError("Set TUCK_FLUIDAUDIO_SOURCE to the verified pinned checkout; this experiment never resolves a remote dependency.")
}
let package = Package(name: "VocabularyQualification", platforms: [.macOS(.v14)],
    products: [.executable(name: "qualify-vocabulary", targets: ["Qualification"])],
    dependencies: [.package(name: "FluidAudio", path: source, traits: [])],
    targets: [.executableTarget(name: "Qualification", dependencies: [.product(name: "FluidAudio", package: "FluidAudio")])])
