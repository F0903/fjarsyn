# Fjarsyn

A serverless screen-sharing and messaging app, using mDNS for local peer discovery and WebRTC for transport.

Since the app operates without a central server, the idea is to be on the same virtual network as your remote peers (using Tailscale for example), or a regular LAN of course.
When both peers boot the app, they will find eachother seemlessly using mDNS, allowing for easy calling or contact setup.

It is also possible to manually call an IP and port, but this is obviously much less secure.

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

#### macOS / Linux

Refer to the [official guide](https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building#dependencies).

When everything is setup up, you should be able to build the project simply by running `cargo build`.
