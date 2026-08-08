use std::{borrow::Cow, collections::HashMap};

use webrtc::{
    api::media_engine::MediaEngine,
    rtp::{extension::HeaderExtension, header::Header},
    rtp_transceiver::{
        rtp_codec::{
            RTCRtpHeaderExtensionCapability, RTCRtpHeaderExtensionParameters, RTCRtpParameters,
            RTPCodecType,
        },
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
    util::{Error as UtilError, Marshal, MarshalSize},
};

use super::super::{Error, ShareEpoch};

pub(super) const URI: &str = "urn:fjarsyn:rtp-hdrext:share-epoch:1";
const WIRE_SIZE: usize = size_of::<u64>();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PacketDisposition {
    DropStale,
    Continue,
    Advance,
}

#[derive(Debug, Clone, Copy)]
struct ShareEpochExtension(ShareEpoch);

impl MarshalSize for ShareEpochExtension {
    fn marshal_size(&self) -> usize {
        WIRE_SIZE
    }
}

impl Marshal for ShareEpochExtension {
    fn marshal_to(&self, buffer: &mut [u8]) -> webrtc::util::Result<usize> {
        if buffer.len() < WIRE_SIZE {
            return Err(UtilError::Other("share epoch extension buffer is too small".into()));
        }
        buffer[..WIRE_SIZE].copy_from_slice(&self.0.value().to_be_bytes());
        Ok(WIRE_SIZE)
    }
}

pub(super) fn register(media_engine: &mut MediaEngine) -> Result<(), Error> {
    media_engine
        .register_header_extension(
            RTCRtpHeaderExtensionCapability { uri: URI.to_owned() },
            RTPCodecType::Video,
            Some(RTCRtpTransceiverDirection::Sendrecv),
        )
        .map_err(|error| Error::WebRtc(error.to_string()))
}

pub(super) fn outbound(epoch: ShareEpoch) -> Result<HeaderExtension, Error> {
    epoch.require_valid()?;
    Ok(HeaderExtension::Custom {
        uri: Cow::Borrowed(URI),
        extension: Box::new(ShareEpochExtension(epoch)),
    })
}

pub(super) fn negotiated_id(parameters: &RTCRtpParameters) -> Result<u8, Error> {
    let mut matches = parameters.header_extensions.iter().filter(|extension| extension.uri == URI);
    let extension = matches.next().ok_or_else(|| {
        Error::Protocol("screen-share epoch RTP extension was not negotiated".into())
    })?;
    if matches.next().is_some() {
        return Err(Error::Protocol(
            "screen-share epoch RTP extension was negotiated more than once".into(),
        ));
    }
    checked_extension_id(extension)
}

fn checked_extension_id(extension: &RTCRtpHeaderExtensionParameters) -> Result<u8, Error> {
    let id = u8::try_from(extension.id)
        .map_err(|_| Error::Protocol("screen-share epoch RTP extension ID is invalid".into()))?;
    if !(1..=14).contains(&id) {
        return Err(Error::Protocol(
            "screen-share epoch RTP extension ID is outside the negotiated one-byte range".into(),
        ));
    }
    Ok(id)
}

pub(super) fn decode(header: &Header, extension_id: u8) -> Result<ShareEpoch, Error> {
    if !header.extension {
        return Err(Error::Protocol("RTP packet has no screen-share epoch extension".into()));
    }
    let mut matches = header.extensions.iter().filter(|extension| extension.id == extension_id);
    let extension = matches
        .next()
        .ok_or_else(|| Error::Protocol("RTP packet has no screen-share epoch extension".into()))?;
    if matches.next().is_some() {
        return Err(Error::Protocol(
            "RTP packet has duplicate screen-share epoch extensions".into(),
        ));
    }
    let bytes: [u8; WIRE_SIZE] = extension
        .payload
        .as_ref()
        .try_into()
        .map_err(|_| Error::Protocol("RTP packet has a malformed screen-share epoch".into()))?;
    let epoch = ShareEpoch::from_value(u64::from_be_bytes(bytes));
    epoch.require_valid()?;
    Ok(epoch)
}

pub(super) fn classify_packet(
    active_epoch: Option<ShareEpoch>,
    packet_epoch: ShareEpoch,
) -> PacketDisposition {
    match active_epoch {
        Some(active_epoch) if packet_epoch < active_epoch => PacketDisposition::DropStale,
        Some(active_epoch) if packet_epoch == active_epoch => PacketDisposition::Continue,
        Some(_) | None => PacketDisposition::Advance,
    }
}

pub(super) fn video_sdp_id(sdp: &str) -> Result<u8, Error> {
    let mut in_video_section = false;
    let mut saw_video_section = false;
    let mut negotiated_id = None;
    let mut extension_ids = HashMap::new();
    for line in sdp.lines().map(str::trim) {
        if line.starts_with("m=") {
            in_video_section = line.starts_with("m=video ");
            if in_video_section {
                if saw_video_section {
                    return Err(Error::Protocol(
                        "SDP contains more than one video media section".into(),
                    ));
                }
                saw_video_section = true;
            }
            continue;
        }
        if !in_video_section {
            continue;
        }
        let Some(mapping) = line.strip_prefix("a=extmap:") else {
            continue;
        };
        let mut fields = mapping.split_ascii_whitespace();
        let Some(id_and_direction) = fields.next() else {
            continue;
        };
        let Some(uri) = fields.next() else {
            continue;
        };
        let direction = id_and_direction.split_once('/').map(|(_, direction)| direction);
        let id = id_and_direction
            .split_once('/')
            .map_or(id_and_direction, |(id, _)| id)
            .parse::<u8>()
            .ok()
            .filter(|id| (1..=14).contains(id));
        let Some(id) = id else {
            if uri == URI {
                return Err(Error::Protocol(
                    "screen-share epoch RTP extension has an invalid SDP ID".into(),
                ));
            }
            continue;
        };
        if extension_ids.insert(id, uri).is_some_and(|previous| previous != uri) {
            return Err(Error::Protocol(
                "video SDP maps one RTP extension ID to multiple URIs".into(),
            ));
        }
        if uri != URI {
            continue;
        }
        if direction.is_some_and(|direction| direction != "sendrecv") {
            return Err(Error::Protocol(
                "screen-share epoch RTP extension is not bidirectional".into(),
            ));
        }
        if negotiated_id.replace(id).is_some() {
            return Err(Error::Protocol(
                "video SDP has duplicate screen-share epoch RTP extensions".into(),
            ));
        }
    }
    negotiated_id
        .ok_or_else(|| Error::Protocol("video SDP has no screen-share epoch RTP extension".into()))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use webrtc::{
        rtp::header::{Extension, Header},
        rtp_transceiver::rtp_codec::{RTCRtpHeaderExtensionParameters, RTCRtpParameters},
        util::Marshal,
    };

    use super::*;

    #[test]
    fn share_epoch_round_trips_and_rejects_malformed_packets() {
        let epoch = ShareEpoch::from_value(42);
        let encoded = ShareEpochExtension(epoch).marshal().unwrap();
        assert_eq!(encoded.as_ref(), 42u64.to_be_bytes());

        let mut header = Header::default();
        header.set_extension(7, encoded).unwrap();
        assert_eq!(decode(&header, 7).unwrap(), epoch);
        assert!(decode(&header, 6).is_err());
        let mut extension_bit_cleared = header.clone();
        extension_bit_cleared.extension = false;
        assert!(decode(&extension_bit_cleared, 7).is_err());

        header.set_extension(7, Bytes::from_static(&[1, 2, 3])).unwrap();
        assert!(decode(&header, 7).is_err());
        header.set_extension(7, Bytes::from_static(&[0; WIRE_SIZE])).unwrap();
        assert!(decode(&header, 7).is_err());

        let encoded = ShareEpochExtension(epoch).marshal().unwrap();
        header.set_extension(7, encoded.clone()).unwrap();
        header.extensions.push(Extension { id: 7, payload: encoded });
        assert!(decode(&header, 7).is_err());
    }

    #[test]
    fn negotiated_extension_id_is_uri_bound_unique_and_nonzero() {
        let parameters = |ids: &[isize]| RTCRtpParameters {
            header_extensions: ids
                .iter()
                .map(|id| RTCRtpHeaderExtensionParameters { uri: URI.into(), id: *id })
                .collect(),
            ..Default::default()
        };
        assert_eq!(negotiated_id(&parameters(&[9])).unwrap(), 9);
        assert_eq!(negotiated_id(&parameters(&[14])).unwrap(), 14);
        assert!(negotiated_id(&parameters(&[])).is_err());
        assert!(negotiated_id(&parameters(&[0])).is_err());
        assert!(negotiated_id(&parameters(&[15])).is_err());
        assert!(negotiated_id(&parameters(&[255])).is_err());
        assert!(negotiated_id(&parameters(&[1, 2])).is_err());
    }

    #[test]
    fn packet_epoch_fence_advances_once_and_never_moves_backwards() {
        let epoch_a = ShareEpoch::FIRST;
        let epoch_b = epoch_a.next().unwrap();

        assert_eq!(classify_packet(None, epoch_a), PacketDisposition::Advance);
        assert_eq!(classify_packet(Some(epoch_a), epoch_a), PacketDisposition::Continue);
        assert_eq!(classify_packet(Some(epoch_a), epoch_b), PacketDisposition::Advance);
        assert_eq!(classify_packet(Some(epoch_b), epoch_a), PacketDisposition::DropStale);
        assert_eq!(classify_packet(Some(epoch_b), epoch_b), PacketDisposition::Continue);
    }

    #[test]
    fn video_sdp_requires_one_bidirectional_epoch_extension() {
        let valid = format!(
            "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:4 {URI}\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n"
        );
        assert_eq!(video_sdp_id(&valid).unwrap(), 4);
        assert!(video_sdp_id("v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n").is_err());
        assert!(
            video_sdp_id(&format!(
                "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:4/recvonly {URI}\r\n"
            ))
            .is_err()
        );
        assert!(
            video_sdp_id(&format!(
                "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:4 {URI}\r\na=extmap:5 {URI}\r\n"
            ))
            .is_err()
        );
        assert!(video_sdp_id(&format!(
            "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:4 {URI}\r\na=extmap:4 urn:ietf:params:rtp-hdrext:sdes:mid\r\n"
        ))
        .is_err());
        assert!(
            video_sdp_id(&format!("m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:15 {URI}\r\n"))
                .is_err()
        );
        assert!(video_sdp_id(&format!(
            "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:4 {URI}\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n"
        ))
        .is_err());
    }
}
