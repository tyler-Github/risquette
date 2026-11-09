<picture>
  <source media="(prefers-color-scheme: dark)" srcset="branding/banner-dark.png">
  <img alt="Risquette: a fast, native desktop client for Discuit" src="branding/banner-light.png">
</picture>

<p align="center">
  <a href="https://github.com/tyler-Github/risquette/releases/latest"><img alt="Download for Windows and Linux" src="https://img.shields.io/badge/Download-Windows%20%7C%20Linux-6b02f2?style=for-the-badge"></a>
  <a href="https://risquette.xnoid.studio"><img alt="Website" src="https://img.shields.io/badge/Website-risquette.xnoid.studio-18102c?style=for-the-badge&logoColor=white"></a>
</p>

<p align="center">
  <a href="https://github.com/tyler-Github/risquette/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/tyler-Github/risquette?include_prereleases&color=6b02f2&label=release"></a>
  <img alt="Built with Rust" src="https://img.shields.io/badge/built%20with-Rust-6b02f2?logo=rust">
  <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-6b02f2">
</p>

<p align="center">
  Browse your <a href="https://discuit.org">Discuit</a> communities, post, comment and get notified,<br>
  in a real desktop app instead of a browser tab.
</p>

<br>

## Highlights

|  |  |
| --- | --- |
| **Native and fast**<br>Written in Rust on [GPUI](https://gpui.rs), the UI framework behind the Zed editor. | **Everything Discuit does**<br>Feeds, posts, threaded comments, notifications, saved lists, mod tools and admin. |
| **Any instance**<br>Use discuit.org or any other Discuit server, with a separate login for each. | **Media done right**<br>Image carousels, plus a pop-out player for YouTube, Vimeo and other videos on Windows. |
| **Light and dark**<br>Themes based on Discuit's own design, following your system. | **Windows and Linux**<br>A per-user installer on Windows and an AppImage on Linux. Both update themselves, and every download is checksum-verified. |

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="branding/framed/app-dark.png">
    <img alt="Risquette showing the All feed" src="branding/framed/app-light.png" width="860">
  </picture>
</p>

## Install

Download Risquette from the [latest release](https://github.com/tyler-Github/risquette/releases/latest). It needs no administrator rights and keeps itself up to date.

- **Windows 10 and 11:** run **RisquetteSetup**. It installs just for you. Windows may show a SmartScreen warning the first time; choose **More info → Run anyway**.
- **Linux (beta):** Linux support is in beta, so for now the AppImage comes with the [newest pre-release](https://risquette.xnoid.studio/releases). Download it, make it executable (`chmod +x Risquette-*.AppImage`, or tick "Allow executing as program" in its properties) and open it. It runs on most distributions and updates itself.

Every release publishes SHA-256 checksums next to the downloads, and [risquette.xnoid.studio/releases](https://risquette.xnoid.studio/releases) lists every version.

## Build from source

```sh
git clone https://github.com/tyler-Github/risquette
cd risquette
cargo run --release
```

You'll need a stable Rust toolchain, plus the MSVC build tools and Windows SDK on Windows, or a few development packages on Linux. See [Development](docs/development.md).

## Documentation

- [Features](docs/features.md): everything Risquette can do, plus keyboard shortcuts
- [Development](docs/development.md): project layout, running, testing
- [Releasing and updates](docs/releasing.md): packaging, the installer and the update server
- [Changelog](CHANGELOG.md): what's new in each release

## Contributing

Bug reports, ideas and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md), or [open an issue](https://github.com/tyler-Github/risquette/issues/new/choose). If Risquette crashes, the crash window can file the report for you.

## Acknowledgements

Risquette is an independent client for [Discuit](https://github.com/discuitnet/discuit) and isn't affiliated with the Discuit project. It's built with [GPUI](https://gpui.rs) and uses [Lucide](https://lucide.dev) icons (ISC).

## License

[Apache-2.0](LICENSE)
