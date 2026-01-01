# rimmich-uploader

A simple Rust-based CLI tool to upload photos and videos to your Immich server.

## Features

- Recursive directory scanning.
- Concurrent uploads (default 3 at a time).
- Automatic MIME type detection.
- Environment variable support for Server URL and API Key.
- Stable `deviceAssetId` generation based on file path.

## Installation

### Binary Releases

You can download pre-compiled binaries for macOS, Windows, and Linux from the [Releases](https://github.com/xufanglin/rimmich-uploader/releases) page.

### From Source

Ensure you have Rust and Cargo installed.

```bash
cargo install --path .
```

## Usage

### Environmental Variables

You can set these in your shell or use the flags:

- `IMMICH_SERVER_URL`: Your Immich server address (e.g., `http://192.168.1.10:2283`)
- `IMMICH_API_KEY`: Your API Key (obtain from Account Settings > API Keys in Immich Web UI)

### User Management (Multi-user support)

You can store multiple users. Use `-u` or `--user` to specify which user to use for an operation.

- **Add a user**:
  ```bash
  rimmich-uploader user add my-user --server http://your-immich:2283 --key your-api-key
  ```
- **Set a default user**:
  ```bash
  rimmich-uploader user default my-user
  ```
- **List users**:
  ```bash
  rimmich-uploader user list
  ```
- **Delete user**:
  ```bash
  rimmich-uploader user delete my-user
  ```

### Usage Examples

- **Using the default user**:
  ```bash
  rimmich-uploader upload /path/to/photos
  ```

- **Specifying a user via flag**:
  ```bash
  rimmich-uploader -u user2 upload /path/to/photos
  ```

- **Manual override (no config needed)**:
  ```bash
  rimmich-uploader --server http://your-server --key your-key upload /path/to/photos
  ```

### Configuration Options

- `--concurrent`: Set number of parallel uploads (default: 10)
- `-r, --recursive`: Enable/disable recursive scanning (default: true)

## GitHub Actions

This project uses GitHub Actions for automatic builds. When a new tag (e.g., `v0.1.0`) is pushed, binaries for the following platforms are automatically built and attached to a new release:
- Linux (x86_64)
- Windows (x86_64)
- macOS (Intel x86_64)
- macOS (Apple Silicon arm64)

## License

MIT
