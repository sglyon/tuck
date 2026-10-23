import Foundation
import ScreenCaptureKit
import CoreMedia
import AVFoundation

// PCM-only stdout: one channel, Float32 little endian, 16 kHz. Diagnostics use stderr.
final class AudioSink: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid else { return }
        var size = 0
        CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(sampleBuffer, bufferListSizeNeededOut: &size, bufferListOut: nil, bufferListSize: 0, blockBufferAllocator: nil, blockBufferMemoryAllocator: nil, flags: 0, blockBufferOut: nil)
        let memory = UnsafeMutableRawPointer.allocate(byteCount: size, alignment: MemoryLayout<AudioBufferList>.alignment)
        defer { memory.deallocate() }
        let list = memory.bindMemory(to: AudioBufferList.self, capacity: 1)
        var retained: CMBlockBuffer?
        guard CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(sampleBuffer, bufferListSizeNeededOut: nil, bufferListOut: list, bufferListSize: size, blockBufferAllocator: nil, blockBufferMemoryAllocator: nil, flags: kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, blockBufferOut: &retained) == noErr else { return }
        for buffer in UnsafeMutableAudioBufferListPointer(list) {
            guard let data = buffer.mData else { continue }
            FileHandle.standardOutput.write(Data(bytes: data, count: Int(buffer.mDataByteSize)))
            break
        }
    }
    func stream(_ stream: SCStream, didStopWithError error: Error) {
        FileHandle.standardError.write(Data("ERROR: System capture stopped. Check System Audio Recording permission.\n".utf8))
        exit(1)
    }
}
@main struct Capture {
    static func main() async {
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
            guard let display = content.displays.first else { throw NSError(domain: "Tuck", code: 1, userInfo: [NSLocalizedDescriptionKey: "No display available for audio capture"])}
            let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
            let config = SCStreamConfiguration()
            config.capturesAudio = true
            config.excludesCurrentProcessAudio = true
            config.sampleRate = 16000
            config.channelCount = 1
            config.width = 2
            config.height = 2
            config.minimumFrameInterval = CMTime(value: 1, timescale: 1)
            let sink = AudioSink()
            let stream = SCStream(filter: filter, configuration: config, delegate: sink)
            try stream.addStreamOutput(sink, type: .audio, sampleHandlerQueue: DispatchQueue(label: "tuck.system-audio", qos: .userInitiated))
            try await stream.startCapture()
            FileHandle.standardError.write(Data("READY\n".utf8))
            // The parent sends EOF on stdin to stop; no screen frames are requested or retained.
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                DispatchQueue.global().async { _ = FileHandle.standardInput.readDataToEndOfFile(); continuation.resume() }
            }
            try await stream.stopCapture()
        } catch {
            FileHandle.standardError.write(Data("ERROR: \(error.localizedDescription)\n".utf8))
            exit(1)
        }
    }
}
