//! Read-only AVFoundation authorization query; never requests permission or opens audio.
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: *const objc2_foundation::NSString;
}

pub fn status() -> &'static str {
    objc2::rc::autoreleasepool(|_| {
        // AVMediaTypeAudio is an immutable framework constant. The class method
        // reports current authorization, including revocations, without prompting.
        let value: isize = unsafe {
            objc2::msg_send![objc2::class!(AVCaptureDevice), authorizationStatusForMediaType: &*AVMediaTypeAudio]
        };
        match value {
            0 => "notDetermined",
            1 => "restricted",
            2 => "denied",
            3 => "authorized",
            _ => "unknown",
        }
    })
}
