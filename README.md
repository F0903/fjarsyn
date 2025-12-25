# Fjarsyn

A work-in-progress screen sharing app.

## Building

To build the project, you first need to follow the sections below:

### Setting Up FFmpeg Build Dependencies

To setup FFmpeg build dependencies, follow the platform-specific instructions below:

#### Windows

To set up the build dependencies for FFmpeg (ffmpeg-next static linking with MSVC buildchain) on Windows, follow these steps:

1. Install LLVM with winget: `winget install --id LLVM.LLVM`
2. Make sure you have vcpkg installed. [(instructions for the bash shell)](https://learn.microsoft.com/en-us/vcpkg/get_started/get-started?pivots=shell-bash#1---set-up-vcpkg)
3. Install FFmpeg for static linking through vcpkg: `vcpkg install ffmpeg:x64-windows-static-md`
4. The project should now be able to build.

#### macOS / Linux

Refer to the [official guide](https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building#dependencies).

When everything is setup up, you should be able to build the project simply by running `cargo build`.
