<picture>
  <source media="(prefers-color-scheme: dark)" srcset="branding/banner-dark.png">
  <img alt="Risquette: a fast, native desktop client for Discuit" src="branding/banner-light.png">
</picture>

<p align="center">
  <a href="https://github.com/tyler-Github/risquette/releases/latest"><img alt="Download" src="https://img.shields.io/badge/Download-Windows-6b02f2?style=for-the-badge&logo=windows11&logoColor=white"></a>
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
| **Any instance**<br>Use discuit.org or any other Discuit server, with a separate login for each. | **Media done right**<br>Image carousels, plus pop-out players for YouTube, Vimeo and other videos. |
| **Light and dark**<br>Themes based on Discuit's own design, following your system. | **Updates itself**<br>A branded installer and updater, with every download checksum-verified. |

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="branding/framed/app-dark.png">
    <img alt="Risquette showing the All feed" src="branding/framed/app-light.png" width="860">
  </picture>
</p>

## Install

Download **RisquetteSetup** from the [latest release](https://github.com/tyler-Github/risquette/releases/latest) and run it. It installs just for you, with no administrator rights, and keeps itself up to date.

Risquette currently targets Windows 10 and 11. Windows may show a SmartScreen warning the first time; choose **More info → Run anyway**. Every release publishes the installer's SHA-256 checksum next to it.

## Build from source

```sh
git clone https://github.com/tyler-Github/risquette
cd risquette
cargo run --release
```

You'll need a stable Rust toolchain, plus the MSVC build tools and Windows SDK on Windows. See [Development](docs/development.md).

## Documentation

- [Features](docs/features.md): everything Risquette can do, plus keyboard shortcuts
- [Development](docs/development.md): project layout, running, testing
- [Releasing and updates](docs/releasing.md): packaging, the installer and the update server

## Contributing

Bug reports, ideas and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md), or [open an issue](https://github.com/tyler-Github/risquette/issues/new/choose). If Risquette crashes, the crash window can file the report for you.

## Acknowledgements

Risquette is an independent client for [Discuit](https://github.com/discuitnet/discuit) and isn't affiliated with the Discuit project. It's built with [GPUI](https://gpui.rs) and uses [Lucide](https://lucide.dev) icons (ISC).

## License

[Apache-2.0](LICENSE)
