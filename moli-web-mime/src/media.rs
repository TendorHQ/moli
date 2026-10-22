use crate::parse::{mime_essence, mime_parameter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaMimeSupport {
    Unsupported,
    Maybe,
    Probably,
}

impl MediaMimeSupport {
    pub fn as_can_play_type(self) -> &'static str {
        match self {
            Self::Unsupported => "",
            Self::Maybe => "maybe",
            Self::Probably => "probably",
        }
    }
}

pub fn media_mime_support(input: &str) -> MediaMimeSupport {
    let Some(mime) = mime_essence(input) else {
        return MediaMimeSupport::Unsupported;
    };
    match mime.as_str() {
        "audio/mp3" | "audio/mpeg" | "audio/webm" | "audio/ogg" | "audio/wav" => {
            MediaMimeSupport::Probably
        }
        "audio/aac" | "audio/flac" => MediaMimeSupport::Maybe,
        "video/mp4" | "video/webm" | "video/ogg" => MediaMimeSupport::Probably,
        _ => MediaMimeSupport::Unsupported,
    }
}

/// Whether the Media Source Extensions playback pipeline is available.
///
/// Keep capability probes on this shared answer until Moli implements the
/// SourceBuffer lifecycle required to use MediaSource for playback.
pub const fn media_source_available() -> bool {
    false
}

/// Reports support only when both the MIME shape and the MSE playback pipeline
/// are available. Recognizing an AVC codec does not make SourceBuffer usable.
pub fn is_media_source_type_supported(input: &str) -> bool {
    media_source_available()
        && mime_essence(input).as_deref() == Some("video/mp4")
        && mime_parameter(input, "codecs").is_some_and(|codecs| {
            codecs
                .split(',')
                .map(str::trim)
                .any(|codec| codec.to_ascii_lowercase().starts_with("avc1."))
        })
}
