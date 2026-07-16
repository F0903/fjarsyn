# Fjarsyn

Fjarsyn is a serverless, contact-oriented screen-sharing app. It uses mDNS to
show which trusted contacts are nearby and an explicitly established WebRTC
session for all application data and media after temporary negotiation.

Presence and connectivity are deliberately separate: seeing a contact as
`Nearby` never opens a network connection. A user chooses **Connect**, the peers
open a TLS 1.3 signaling connection pinned to the saved contact identity,
authenticate the signed WebRTC negotiation, and mark the contact `Connected`
only after the required encrypted channels are ready. Chat then uses a WebRTC
data channel, while screen sharing uses the session's media tracks. There is no
separate call mode or manual address flow.

Peers exchange copyable pairing invites and compare the displayed identity
fingerprint over an independent trusted channel before saving one another as
contacts. Pairing is reciprocal; a QR code is not required.

The peers need network reachability to one another, such as a shared LAN or an
overlay network that carries mDNS and peer-to-peer traffic.

The accepted runtime and security boundaries are documented in
[`docs/architecture/peer-sessions.md`](docs/architecture/peer-sessions.md).

## Building

To build the project, you first need to follow the sections below:

### Setting Up FFmpeg Build Dependencies

To setup FFmpeg build dependencies, follow the platform-specific instructions below:

#### Windows

To set up the build dependencies for FFmpeg (ffmpeg-next static linking with MSVC buildchain) on Windows, follow these steps:

1. Install LLVM with winget: `winget install --id LLVM.LLVM`
2. Make sure you have vcpkg installed. [(instructions for the bash shell)](https://learn.microsoft.com/en-us/vcpkg/get_started/get-started?pivots=shell-bash#1---set-up-vcpkg)
3. Install FFmpeg for static linking through vcpkg: `vcpkg install ffmpeg[core,avcodec,avformat,swresample,swscale,gpl,x264,x265,aom,dav1d,nvcodec] --triplet x64-windows-static-md --recurse`
4. The project should now be able to build.

Fjarsyn currently targets Windows because screen capture and GPU interop use
Windows Graphics Capture, D3D11 and D3D12.

After installing the native dependencies, build the workspace with
`cargo build --workspace`.
