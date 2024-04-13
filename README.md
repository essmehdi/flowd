# Flow daemon

A download manager daemon for Linux written in Rust. This is part of the major architecture overhaul I am doing to [Flow](https://github.com/essmehdi/flow) project. The aim is to make the download manager distribution and desktop environment agnostic and everyone can create a GUI to it for their favorite desktop environment or TUIs.

The GUI I will support is a GNOME application.

**This project is still in early development and unstable.**

Docs are coming soon! :)

## Features

- [x] Manage downloads
- [x] Auto-categorize downloads 
- [ ] Queuing and scheduling
- [ ] Multi-connections downloads
- [ ] Support more protocols

## API

To communicate with the daemon, it exposes a DBus interface. You will be able to manage downloads through it.

## Installation

Compile the daemon, then run the installation script

```sh
cargo build --release
./install.py
```

`install.py` takes an argument `--install-path` or `-i` to change the installation path for the binary. The default path is `/usr/local/bin`