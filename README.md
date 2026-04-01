A KRunner plugin / "runner" that lists Zed's recent workspaces

- Quickly re-open the workspace in Zed by pressing `Enter`
- Supports: Stable, Dev versions of Zed


## Screenshot

![Screenshot](krunner-zed-demo.png)

## Requirements

- kstart
  - Arch: `sudo pacman -S kde-cli-tools`

## Building

```bash
cargo build --release
```

## Install plugin

```bash
cp target/release/krunner-zed package/krunner-zed && package/install.sh
```

## Uninstall plugin

```bash
package/uninstall.sh
```
